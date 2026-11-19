//! Wayland adapter for the keyboard-driven selection state machine.

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, SurfaceData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use wayland_client::{
    Connection, Proxy, QueueHandle, delegate_noop,
    globals::registry_queue_init,
    protocol::{self, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

use crate::{
    bisect::{BisectState, Rect},
    config::Config,
    input::{InputAction, KeyMap, MouseButton},
};

mod render;
use render::Overlay;

const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;
const BUTTON_MIDDLE: u32 = 0x112;

#[derive(Debug, thiserror::Error)]
pub enum WaylandError {
    #[error("could not connect to the Wayland compositor: {0}")]
    Connection(#[source] wayland_client::ConnectError),
    #[error("Wayland registry initialization failed: {0}")]
    Registry(#[source] wayland_client::globals::GlobalError),
    #[error("required Wayland global is unavailable: {0}")]
    Global(String),
    #[error("could not prepare a shared-memory drawing buffer: {0}")]
    Buffer(String),
    #[error("Wayland dispatch failed: {0}")]
    Dispatch(String),
    #[error("no Wayland virtual pointer is available")]
    PointerUnavailable,
    #[error("cannot map the virtual pointer to the overlay's output")]
    PointerOutputUnavailable,
}

/// Run the interactive overlay until the user clicks, cancels, or the compositor closes it.
pub fn run(config: Config) -> Result<(), WaylandError> {
    let connection = Connection::connect_to_env().map_err(WaylandError::Connection)?;
    let (globals, mut event_queue) =
        registry_queue_init::<Binger>(&connection).map_err(WaylandError::Registry)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| WaylandError::Global(format!("wl_compositor: {error}")))?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|error| WaylandError::Global(format!("zwlr_layer_shell_v1: {error}")))?;
    let shm = Shm::bind(&globals, &qh)
        .map_err(|error| WaylandError::Global(format!("wl_shm: {error}")))?;
    let virtual_pointer_manager = globals
        .bind::<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, _, _>(&qh, 1..=2, ())
        .map_err(|error| {
            WaylandError::Global(format!("zwlr_virtual_pointer_manager_v1: {error}"))
        })?;

    let registry_state = RegistryState::new(&globals);
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);
    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("binger"), None);
    layer.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM | Anchor::LEFT);
    layer.set_size(0, 0);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);

    // The overlay receives keyboard focus while preserving pointer input for the application below it.
    let input_region = compositor.wl_compositor().create_region(&qh, ());
    layer.wl_surface().set_input_region(Some(&input_region));
    input_region.destroy();
    layer.commit();

    let pool = SlotPool::new(4 * 1024 * 1024, &shm)
        .map_err(|error| WaylandError::Buffer(error.to_string()))?;
    let key_map = config
        .key_map()
        .map_err(|error| WaylandError::Global(format!("invalid key map: {error}")))?;

    let mut state = Binger {
        registry_state,
        seat_state,
        output_state,
        shm,
        layer,
        pool,
        buffer: None,
        keyboard: None,
        keyboard_focus: false,
        virtual_pointer_manager,
        virtual_pointer: None,
        selection: None,
        ratio: config.bisect_ratio,
        key_map,
        overlay: Overlay::new(&config),
        scale_factor: 1,
        exit: None,
    };

    let exit = loop {
        event_queue
            .blocking_dispatch(&mut state)
            .map_err(|error| WaylandError::Dispatch(error.to_string()))?;
        if let Some(exit) = state.exit.take() {
            break exit;
        }
    };

    match exit {
        Exit::Cancel => Ok(()),
        Exit::Error(error) => Err(error),
        Exit::Click(button, time) => {
            let (area, bounds) = state
                .selection
                .as_ref()
                .ok_or(WaylandError::PointerUnavailable)?;
            let pointer = state
                .virtual_pointer
                .as_ref()
                .ok_or(WaylandError::PointerUnavailable)?;
            // Restore focus before moving to the click target.
            drop(state.layer);
            sync(&connection)?;
            move_pointer(pointer, area.current(), *bounds, time);
            sync(&connection)?;
            click(pointer, button, time);
            sync(&connection)
        }
    }
}

fn sync(connection: &Connection) -> Result<(), WaylandError> {
    connection
        .roundtrip()
        .map_err(|error| WaylandError::Dispatch(error.to_string()))?;
    Ok(())
}

enum Exit {
    Cancel,
    Click(MouseButton, u32),
    Error(WaylandError),
}

struct Binger {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    layer: LayerSurface,
    pool: SlotPool,
    buffer: Option<Buffer>,
    keyboard: Option<(wl_seat::WlSeat, wl_keyboard::WlKeyboard)>,
    keyboard_focus: bool,
    virtual_pointer_manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    virtual_pointer: Option<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1>,
    selection: Option<(BisectState, (u32, u32))>,
    key_map: KeyMap,
    overlay: Overlay,
    scale_factor: u32,
    exit: Option<Exit>,
    ratio: f32,
}

impl CompositorHandler for Binger {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        self.scale_factor = new_factor.max(1) as u32;
        self.redraw();
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        if let Some(pointer) = self.virtual_pointer.take() {
            pointer.destroy();
        }
    }
}

impl OutputHandler for Binger {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.overlay_output().as_ref() == Some(&output) {
            self.exit.get_or_insert(Exit::Cancel);
        }
    }
}

impl LayerShellHandler for Binger {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit.get_or_insert(Exit::Cancel);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let size = self.configured_size(configure.new_size);
        if self
            .selection
            .as_ref()
            .is_none_or(|(_, bounds)| *bounds != size)
        {
            let (width, height) = size;
            self.selection = Some((
                BisectState::new(Rect::new(0, 0, width, height), self.ratio),
                size,
            ));
        }
        self.redraw();
    }
}

impl SeatHandler for Binger {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            if let Ok(keyboard) = self.seat_state.get_keyboard(qh, &seat, None) {
                self.keyboard = Some((seat, keyboard));
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && self
                .keyboard
                .as_ref()
                .is_some_and(|(bound, _)| *bound == seat)
        {
            self.keyboard_focus = false;
            if let Some((_, keyboard)) = self.keyboard.take() {
                if keyboard.version() >= 3 {
                    keyboard.release();
                }
            }
            if let Some(pointer) = self.virtual_pointer.take() {
                pointer.destroy();
            }
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl KeyboardHandler for Binger {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        self.keyboard_focus = self.layer.wl_surface() == surface;
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        if self.layer.wl_surface() == surface {
            self.keyboard_focus = false;
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.handle_key(qh, event);
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
    }
}

impl ShmHandler for Binger {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl Binger {
    fn handle_key(&mut self, qh: &QueueHandle<Self>, event: KeyEvent) {
        if !self.keyboard_focus || self.exit.is_some() {
            return;
        }
        if event.keysym == Keysym::Escape {
            self.exit = Some(Exit::Cancel);
            return;
        }
        let Some((selection, _)) = self.selection.as_mut() else {
            return;
        };
        if event.keysym == Keysym::BackSpace {
            if !selection.undo() {
                return;
            }
        } else {
            let Some(character) = event.keysym.key_char() else {
                return;
            };
            let action = match self.key_map.action_for(character) {
                InputAction::Unknown if character.is_ascii_uppercase() => {
                    self.key_map.action_for(character.to_ascii_lowercase())
                }
                action => action,
            };
            match action {
                InputAction::SelectArea(index) => {
                    if selection.select(index).is_err() {
                        return;
                    }
                }
                InputAction::Click(button) => {
                    self.exit = Some(match self.ensure_pointer(qh) {
                        Ok(()) => Exit::Click(button, event.time),
                        Err(error) => Exit::Error(error),
                    });
                    return;
                }
                InputAction::Unknown => return,
            }
        }
        if let Err(error) = self.ensure_pointer(qh) {
            self.exit = Some(Exit::Error(error));
            return;
        }
        if let (Some(pointer), Some((selection, bounds))) = (&self.virtual_pointer, &self.selection)
        {
            move_pointer(pointer, selection.current(), *bounds, event.time);
        }
        self.redraw();
    }

    fn overlay_output(&self) -> Option<wl_output::WlOutput> {
        self.layer
            .wl_surface()
            .data::<SurfaceData<()>>()
            .and_then(|data| data.outputs().next())
    }

    fn ensure_pointer(&mut self, qh: &QueueHandle<Self>) -> Result<(), WaylandError> {
        if self.virtual_pointer.is_some() {
            return Ok(());
        }
        let (seat, _) = self
            .keyboard
            .as_ref()
            .ok_or(WaylandError::PointerUnavailable)?;
        let manager = &self.virtual_pointer_manager;
        let output = self.overlay_output();
        let pointer = match (manager.version(), output.as_ref()) {
            (2.., Some(output)) => {
                manager.create_virtual_pointer_with_output(Some(seat), Some(output), qh, ())
            }
            _ if self.output_state.outputs().count() <= 1 => {
                manager.create_virtual_pointer(Some(seat), qh, ())
            }
            _ => return Err(WaylandError::PointerOutputUnavailable),
        };
        self.virtual_pointer = Some(pointer);
        Ok(())
    }

    fn configured_size(&self, (width, height): (u32, u32)) -> (u32, u32) {
        if width != 0 && height != 0 {
            return (width, height);
        }
        let (fallback_width, fallback_height) = self
            .overlay_output()
            .or_else(|| self.output_state.outputs().next())
            .as_ref()
            .and_then(|output| self.output_state.info(output))
            .and_then(|info| info.logical_size)
            .unwrap_or((1920, 1080));
        (
            if width == 0 {
                fallback_width.max(1) as u32
            } else {
                width
            },
            if height == 0 {
                fallback_height.max(1) as u32
            } else {
                height
            },
        )
    }

    fn redraw(&mut self) {
        if self.exit.is_none() {
            self.exit = self.draw().err().map(Exit::Error);
        }
    }

    fn draw(&mut self) -> Result<(), WaylandError> {
        let Some((selection, (width, height))) = self.selection.as_ref() else {
            return Ok(());
        };
        let scale = self.scale_factor;
        let width = width.saturating_mul(scale);
        let height = height.saturating_mul(scale);
        if u64::from(width) * u64::from(height) > i32::MAX as u64 / 4 {
            return Err(WaylandError::Buffer(format!(
                "invalid size {width}×{height}"
            )));
        }
        let (width_px, height_px) = (width as i32, height as i32);
        let stride = width_px * 4;
        let (buffer, canvas) = match self.buffer.take() {
            Some(buffer)
                if buffer.stride() == stride
                    && buffer.height() == height_px
                    && !buffer.slot().has_active_buffers() =>
            {
                let canvas = self
                    .pool
                    .canvas(&buffer)
                    .expect("buffer belongs to this pool");
                (buffer, canvas)
            }
            _ => self
                .pool
                .create_buffer(width_px, height_px, stride, wl_shm::Format::Argb8888)
                .map_err(|error| WaylandError::Buffer(error.to_string()))?,
        };

        self.overlay
            .draw(canvas, width, height, scale, selection, &self.key_map);
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(scale as i32);
        buffer
            .attach_to(surface)
            .map_err(|error| WaylandError::Buffer(error.to_string()))?;
        surface.damage_buffer(0, 0, width_px, height_px);
        self.layer.commit();
        self.buffer = Some(buffer);
        Ok(())
    }
}

fn move_pointer(
    pointer: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    area: Rect,
    (width, height): (u32, u32),
    time: u32,
) {
    let (x, y) = area.center();
    pointer.motion_absolute(time, x as u32, y as u32, width, height);
    pointer.frame();
}

fn click(pointer: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, button: MouseButton, time: u32) {
    let code = match button {
        MouseButton::Left => BUTTON_LEFT,
        MouseButton::Right => BUTTON_RIGHT,
        MouseButton::Middle => BUTTON_MIDDLE,
    };
    pointer.button(time, code, wl_pointer::ButtonState::Pressed);
    pointer.frame();
    pointer.button(time, code, wl_pointer::ButtonState::Released);
    pointer.frame();
}

delegate_noop!(Binger: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
delegate_noop!(Binger: zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);
delegate_noop!(Binger: protocol::wl_region::WlRegion);

delegate_registry!(Binger);

impl ProvidesRegistryState for Binger {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Binger);
