//! Wayland adapter for the keyboard-driven selection state machine.

use std::{
    env,
    io::Read,
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, SurfaceData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers, RepeatInfo},
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
    let (globals, event_queue) =
        registry_queue_init::<Binger>(&connection).map_err(WaylandError::Registry)?;
    let qh = event_queue.handle();
    let mut event_loop =
        EventLoop::try_new().map_err(|error| WaylandError::Dispatch(error.to_string()))?;
    WaylandSource::new(connection.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|error| WaylandError::Dispatch(error.to_string()))?;

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
        repeat_info: RepeatInfo::Disable,
        nudge_repeats: Vec::new(),
        virtual_pointer_manager,
        virtual_pointer: None,
        selection: None,
        ratio: config.bisect_ratio,
        key_map,
        nudge_step: config.nudge_step,
        nudge_interval: Duration::from_millis(config.nudge_interval_ms),
        nudge_acceleration_interval: Duration::from_millis(config.nudge_acceleration_interval_ms),
        nudge_max_step: config.nudge_max_step,
        overlay: Overlay::new(&config),
        scale_factor: 1,
        exit: None,
        hyprland_event_buffer: String::new(),
        focus_recovery_pending: false,
    };

    if let Some(socket) = hyprland_event_socket() {
        let source = Generic::new(socket, Interest::READ, Mode::Level);
        event_loop
            .handle()
            .insert_source(source, |readiness, socket, state: &mut Binger| {
                if !readiness.readable {
                    return Ok(PostAction::Continue);
                }
                Ok(state.read_hyprland_events(socket))
            })
            .map_err(|error| WaylandError::Dispatch(error.to_string()))?;
    }

    let exit = loop {
        let timeout = state.next_nudge_timeout();
        event_loop
            .dispatch(timeout, &mut state)
            .map_err(|error| WaylandError::Dispatch(error.to_string()))?;
        if state.focus_recovery_pending {
            state.focus_recovery_pending = false;
            state.reclaim_keyboard_focus();
        }
        state.repeat_nudge(&qh);
        if let Some(exit) = state.exit.take() {
            break exit;
        }
    };

    match exit {
        Exit::Cancel => Ok(()),
        Exit::Error(error) => Err(error),
        Exit::Click {
            button,
            time,
            target,
            bounds,
        } => {
            let pointer = state
                .virtual_pointer
                .take()
                .ok_or(WaylandError::PointerUnavailable)?;
            // Restore focus before moving to the click target.
            drop(state);
            sync(&connection)?;
            move_pointer(&pointer, target, bounds, time);
            sync(&connection)?;
            click(&pointer, button, time);
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
    Click {
        button: MouseButton,
        time: u32,
        target: (i32, i32),
        bounds: Rect,
    },
    Error(WaylandError),
}

fn hyprland_event_socket() -> Option<UnixStream> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")?;
    let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let path = PathBuf::from(runtime_dir)
        .join("hypr")
        .join(signature)
        .join(".socket2.sock");
    let socket = UnixStream::connect(path).ok()?;
    socket.set_nonblocking(true).ok()?;
    Some(socket)
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
    repeat_info: RepeatInfo,
    nudge_repeats: Vec<NudgeRepeat>,
    virtual_pointer_manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    virtual_pointer: Option<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1>,
    selection: Option<BisectState>,
    key_map: KeyMap,
    overlay: Overlay,
    scale_factor: u32,
    exit: Option<Exit>,
    ratio: f32,
    hyprland_event_buffer: String,
    focus_recovery_pending: bool,
    nudge_step: i32,
    nudge_interval: Duration,
    nudge_acceleration_interval: Duration,
    nudge_max_step: i32,
}

struct NudgeRepeat {
    raw_code: u32,
    direction: (i32, i32),
    press_time: u32,
    pressed_at: Instant,
    first_repeat: Option<Instant>,
    next: Option<Instant>,
    interval: Duration,
    acceleration_interval: Duration,
    max_step: i32,
}

impl NudgeRepeat {
    fn new(
        event: &KeyEvent,
        direction: (i32, i32),
        info: RepeatInfo,
        interval: Duration,
        acceleration_interval: Duration,
        max_step: i32,
    ) -> Self {
        let pressed_at = Instant::now();
        let next = match info {
            RepeatInfo::Repeat { delay, .. } => {
                Some(pressed_at + Duration::from_millis(delay.into()))
            }
            RepeatInfo::Disable => None,
        };
        Self {
            raw_code: event.raw_code,
            direction,
            press_time: event.time,
            pressed_at,
            first_repeat: None,
            next,
            interval,
            acceleration_interval,
            max_step,
        }
    }

    fn tick(&mut self, now: Instant) -> Option<(i32, u32)> {
        let due = self.next.filter(|&due| due <= now)?;
        let next = due + self.interval;
        // Skip missed ticks so a stalled frame cannot cause a sudden jump.
        self.next = Some(if next <= now {
            now + self.interval
        } else {
            next
        });

        let first_repeat = *self.first_repeat.get_or_insert(now);
        let held = now.duration_since(first_repeat);
        let steps = 1 + held.as_millis() / self.acceleration_interval.as_millis();
        let step = steps.min(self.max_step as u128) as i32;
        let elapsed = now.duration_since(self.pressed_at).as_millis() as u32;
        Some((step, self.press_time.wrapping_add(elapsed)))
    }
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
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        self.move_pointer_if_ready(qh, 0);
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
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (width, height) = self.configured_size(configure.new_size);
        let bounds = Rect::new(0, 0, width, height);
        if self
            .selection
            .as_ref()
            .is_none_or(|selection| selection.bounds() != bounds)
        {
            self.selection = Some(BisectState::new(bounds, self.ratio));
        }
        self.move_pointer_if_ready(qh, 0);
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
                self.move_pointer_if_ready(qh, 0);
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
            self.nudge_repeats.clear();
            self.repeat_info = RepeatInfo::Disable;
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
        self.nudge_repeats.clear();
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
            self.nudge_repeats.clear();
            if self.focus_recovery_pending {
                self.focus_recovery_pending = false;
                self.reclaim_keyboard_focus();
            }
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
        self.handle_press(qh, &event);
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if self.exit.is_some() || !self.keyboard_focus {
            return;
        }
        if let Some(repeat) = self
            .nudge_repeats
            .iter_mut()
            .find(|repeat| repeat.raw_code == event.raw_code)
        {
            // A compositor-driven repeat starts the same timer used by client-side repeat.
            repeat.next.get_or_insert_with(Instant::now);
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.nudge_repeats
            .retain(|repeat| repeat.raw_code != event.raw_code);
    }

    fn update_repeat_info(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        info: RepeatInfo,
    ) {
        self.repeat_info = info;
        self.nudge_repeats.clear();
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
    fn read_hyprland_events(&mut self, mut socket: &UnixStream) -> PostAction {
        let mut buffer = [0_u8; 4096];
        match socket.read(&mut buffer) {
            Ok(0) => PostAction::Remove,
            Ok(size) => {
                self.hyprland_event_buffer
                    .push_str(&String::from_utf8_lossy(&buffer[..size]));
                while let Some(newline) = self.hyprland_event_buffer.find('\n') {
                    let event = self.hyprland_event_buffer[..newline].trim_end();
                    if event.starts_with("closelayer>>") {
                        self.focus_recovery_pending = true;
                    }
                    self.hyprland_event_buffer.drain(..=newline);
                }
                PostAction::Continue
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                PostAction::Continue
            }
            Err(_) => PostAction::Remove,
        }
    }

    fn reclaim_keyboard_focus(&mut self) {
        if self.exit.is_some() || self.keyboard_focus || self.keyboard.is_none() {
            return;
        }
        self.layer
            .set_keyboard_interactivity(KeyboardInteractivity::None);
        self.layer.commit();
        self.layer
            .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        self.layer.commit();
    }

    fn next_nudge_timeout(&self) -> Option<Duration> {
        self.nudge_repeats
            .iter()
            .filter_map(|repeat| repeat.next)
            .min()
            .map(|next| next.saturating_duration_since(Instant::now()))
    }

    fn repeat_nudge(&mut self, qh: &QueueHandle<Self>) {
        if self.exit.is_some() || !self.keyboard_focus {
            return;
        }
        if self.nudge_repeats.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut delta = (0_i32, 0_i32);
        let mut time = None;
        for repeat in &mut self.nudge_repeats {
            let Some((step, event_time)) = repeat.tick(now) else {
                continue;
            };
            let step = step.saturating_mul(self.nudge_step);
            delta.0 = delta
                .0
                .saturating_add(repeat.direction.0.saturating_mul(step));
            delta.1 = delta
                .1
                .saturating_add(repeat.direction.1.saturating_mul(step));
            time = Some(event_time);
        }
        if let Some(time) = time {
            self.nudge(qh, delta, 1, time);
        }
    }

    fn handle_press(&mut self, qh: &QueueHandle<Self>, event: &KeyEvent) {
        if self.exit.is_some() {
            return;
        }
        if event.keysym == Keysym::Escape {
            self.exit = Some(Exit::Cancel);
            return;
        }
        if !self.keyboard_focus {
            return;
        }
        let Some(selection) = self.selection.as_mut() else {
            return;
        };
        let changed = if event.keysym == Keysym::BackSpace {
            self.nudge_repeats.clear();
            selection.undo()
        } else {
            let action = match event.keysym {
                Keysym::Return | Keysym::KP_Enter | Keysym::space | Keysym::KP_Space => {
                    InputAction::Click(MouseButton::Left)
                }
                _ => {
                    let Some(character) = event.keysym.key_char() else {
                        return;
                    };
                    self.key_map.action_for(character)
                }
            };
            match action {
                InputAction::Nudge { dx, dy } => {
                    self.nudge_repeats
                        .retain(|repeat| repeat.raw_code != event.raw_code);
                    self.nudge_repeats.push(NudgeRepeat::new(
                        event,
                        (dx, dy),
                        self.repeat_info,
                        self.nudge_interval,
                        self.nudge_acceleration_interval,
                        self.nudge_max_step,
                    ));
                    selection.nudge(dx * self.nudge_step, dy * self.nudge_step)
                }
                InputAction::SelectArea(index) => {
                    self.nudge_repeats.clear();
                    selection.select(index).is_ok()
                }
                InputAction::Click(button) => {
                    self.nudge_repeats.clear();
                    let target = selection.target();
                    let bounds = selection.bounds();
                    self.exit = Some(match self.ensure_pointer(qh) {
                        Ok(_) => Exit::Click {
                            button,
                            time: event.time,
                            target,
                            bounds,
                        },
                        Err(error) => Exit::Error(error),
                    });
                    return;
                }
                InputAction::Unknown => {
                    self.nudge_repeats.clear();
                    return;
                }
            }
        };
        if changed {
            self.update_pointer(qh, event.time);
        }
    }

    fn nudge(&mut self, qh: &QueueHandle<Self>, (dx, dy): (i32, i32), step: i32, time: u32) {
        if self
            .selection
            .as_mut()
            .is_some_and(|selection| selection.nudge(dx * step, dy * step))
        {
            self.update_pointer(qh, time);
        }
    }

    fn update_pointer(&mut self, qh: &QueueHandle<Self>, time: u32) {
        if self.move_pointer_if_ready(qh, time) {
            self.redraw();
        }
    }

    fn move_pointer_if_ready(&mut self, qh: &QueueHandle<Self>, time: u32) -> bool {
        if self.keyboard.is_none() || self.selection.is_none() {
            return false;
        }
        if self.overlay_output().is_none() && self.output_state.outputs().count() != 1 {
            return false;
        }
        let Some(selection) = &self.selection else {
            return false;
        };
        let target = selection.target();
        let bounds = selection.bounds();
        match self.ensure_pointer(qh) {
            Ok(pointer) => move_pointer(pointer, target, bounds, time),
            Err(error) => {
                self.exit = Some(Exit::Error(error));
            }
        }
        true
    }

    fn overlay_output(&self) -> Option<wl_output::WlOutput> {
        self.layer
            .wl_surface()
            .data::<SurfaceData<()>>()
            .and_then(|data| data.outputs().next())
    }

    fn ensure_pointer(
        &mut self,
        qh: &QueueHandle<Self>,
    ) -> Result<&zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, WaylandError> {
        if self.virtual_pointer.is_none() {
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
        }
        self.virtual_pointer
            .as_ref()
            .ok_or(WaylandError::PointerUnavailable)
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
        let Some(selection) = self.selection.as_ref() else {
            return Ok(());
        };
        let Rect { width, height, .. } = selection.bounds();
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
        let (buffer, canvas, reuse_buffer) = match self.buffer.take() {
            Some(buffer)
                if buffer.stride() == stride
                    && buffer.height() == height_px
                    && !buffer.slot().has_active_buffers() =>
            {
                let canvas = self
                    .pool
                    .canvas(&buffer)
                    .expect("buffer belongs to this pool");
                (buffer, canvas, true)
            }
            _ => self
                .pool
                .create_buffer(width_px, height_px, stride, wl_shm::Format::Argb8888)
                .map(|(buffer, canvas)| (buffer, canvas, false))
                .map_err(|error| WaylandError::Buffer(error.to_string()))?,
        };

        let damage = self.overlay.draw(
            canvas,
            (width, height),
            scale,
            selection,
            &self.key_map,
            reuse_buffer,
        );
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(scale as i32);
        buffer
            .attach_to(surface)
            .map_err(|error| WaylandError::Buffer(error.to_string()))?;
        match damage {
            Some(areas) => {
                for area in areas {
                    surface.damage_buffer(area.x, area.y, area.width as i32, area.height as i32);
                }
            }
            None => surface.damage_buffer(0, 0, width_px, height_px),
        }
        self.layer.commit();
        self.buffer = Some(buffer);
        Ok(())
    }
}

fn move_pointer(
    pointer: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    (x, y): (i32, i32),
    bounds: Rect,
    time: u32,
) {
    pointer.motion_absolute(time, x as u32, y as u32, bounds.width, bounds.height);
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

#[cfg(test)]
mod tests {
    use super::{Duration, KeyEvent, Keysym, NudgeRepeat, RepeatInfo};

    #[test]
    fn nudge_accelerates_at_a_fixed_cadence_and_skips_missed_ticks() {
        let event = KeyEvent {
            time: 0,
            raw_code: 0,
            keysym: Keysym::h,
            utf8: None,
        };
        let mut repeat = NudgeRepeat::new(
            &event,
            (-1, 0),
            RepeatInfo::Disable,
            Duration::from_millis(15),
            Duration::from_millis(160),
            6,
        );
        let start = repeat.pressed_at + Duration::from_millis(80);
        assert_eq!(repeat.tick(start), None);
        repeat.next = Some(start);
        assert_eq!(repeat.tick(start), Some((1, 80)));
        assert_eq!(repeat.tick(start + Duration::from_millis(14)), None);

        for (tick, step) in [(10, 1), (11, 2), (22, 3), (32, 4), (43, 5), (54, 6)] {
            let elapsed = Duration::from_millis(15 * tick);
            assert_eq!(
                repeat.tick(start + elapsed),
                Some((step, 80 + elapsed.as_millis() as u32))
            );
        }

        let late = start + Duration::from_secs(3);
        assert_eq!(repeat.tick(late), Some((6, 3080)));
        assert_eq!(repeat.tick(late), None);
        assert_eq!(repeat.next, Some(late + Duration::from_millis(15)));
    }
}
