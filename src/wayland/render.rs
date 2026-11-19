use fontdb::{Database, Family, Query};
use fontdue::{Font, FontSettings, Metrics};

use crate::{
    bisect::{BisectState, Division, Rect},
    config::Config,
    input::KeyMap,
};

const TEAL: [u8; 3] = [32, 82, 96];
const PLUM: [u8; 3] = [91, 57, 96];
const TEAL_EDGE: [u8; 3] = [111, 195, 214];
const PLUM_EDGE: [u8; 3] = [198, 145, 207];
const LABEL: [u8; 3] = [246, 248, 250];
const MAX_CELL_FONT_SIZE: f32 = 512.0;
const CROSSHAIR: [u8; 3] = [255, 213, 124];

pub(super) struct Overlay {
    style: GridStyle,
    background: Vec<u8>,
    previous_frame: Option<(GridLayout, Rect)>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct GridLayout {
    size: (u32, u32),
    scale: u32,
    area: Rect,
}

struct GridStyle {
    font: Option<Font>,
    font_size: u32,
    opacity: u8,
}

impl Overlay {
    pub(super) fn new(config: &Config) -> Self {
        Self {
            style: GridStyle {
                font: load_font(&config.label_font_family),
                font_size: config.label_font_size.max(1),
                opacity: (config.overlay_opacity * 255.0).round() as u8,
            },
            background: Vec::new(),
            previous_frame: None,
        }
    }

    pub(super) fn draw(
        &mut self,
        pixels: &mut [u8],
        size: (u32, u32),
        scale: u32,
        selection: &BisectState,
        keys: &KeyMap,
        reuse_buffer: bool,
    ) -> Option<[Rect; 2]> {
        let layout = GridLayout {
            size,
            scale,
            area: selection.current(),
        };
        let previous_crosshair = self
            .previous_frame
            .filter(|&(previous, _)| previous == layout)
            .map(|(_, crosshair)| crosshair);
        let (width, height) = size;
        if previous_crosshair.is_none() {
            self.background.resize(pixels.len(), 0);
            self.background.fill(0);
            let mut background = Canvas {
                pixels: &mut self.background,
                width,
                height,
            };
            self.style.draw(&mut background, selection, keys, scale);
        }

        let mut canvas = Canvas {
            pixels,
            width,
            height,
        };
        if let Some(previous) = previous_crosshair.filter(|_| reuse_buffer) {
            canvas.restore(previous, &self.background);
        } else {
            canvas.pixels.copy_from_slice(&self.background);
        }

        let (x, y) = selection.target();
        let arm = 4 * scale;
        let x = x * scale as i32;
        let y = y * scale as i32;
        let length = 2 * arm + scale;
        let left = x - arm as i32;
        let top = y - arm as i32;
        let color = argb(230, CROSSHAIR);
        canvas.fill(Rect::new(left, y, length, scale), color);
        canvas.fill(Rect::new(x, top, scale, length), color);
        let crosshair = Rect::new(left, top, length, length);
        self.previous_frame = Some((layout, crosshair));
        previous_crosshair.map(|previous| [previous, crosshair])
    }
}

impl GridStyle {
    fn draw(&self, canvas: &mut Canvas<'_>, selection: &BisectState, keys: &KeyMap, scale: u32) {
        let selected = scaled(selection.current(), scale);
        let division = selection.division();
        let slots = division.slots();
        let size = canvas.width.min(canvas.height) / scale;
        let font_size = self.font_size.min(size).max(1) as f32 * scale as f32;

        let outside = (0..slots).any(|index| {
            let cell = selection.subarea_for(division, index);
            let height = (cell.height * scale) as f32;
            let width = (cell.width * scale) as f32;
            height <= font_size || width <= font_size
        });
        for index in 0..slots {
            let cell = selection.subarea_for(division, index);
            let (column, row) = division.cell(index);
            let (fill, edge) = if (column + row) % 2 == 0 {
                (TEAL, TEAL_EDGE)
            } else {
                (PLUM, PLUM_EDGE)
            };
            let area = scaled(cell, scale);
            canvas.fill(area, argb(self.opacity, fill));
            canvas.outline(area, scale, argb(200, edge));
            if let (false, Some(font), Some(key)) = (outside, &self.font, keys.area_key(index)) {
                let size = (area.height as f32 * 0.88)
                    .min(area.width as f32)
                    .min(MAX_CELL_FONT_SIZE * scale as f32)
                    .max(font_size);
                canvas.label(font, key, area.center(), size);
            }
        }
        if let (true, Some(font)) = (outside, &self.font) {
            canvas.outside_labels(font, keys, division, selected, font_size, scale);
        }
    }
}

struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: u32,
    height: u32,
}

impl Canvas<'_> {
    fn restore(&mut self, area: Rect, background: &[u8]) {
        let Some(area) = self.clip(area) else {
            return;
        };
        let stride = self.width as usize * 4;
        let start = area.y as usize * stride + area.x as usize * 4;
        for y in 0..area.height as usize {
            let start = start + y * stride;
            let row = start..start + area.width as usize * 4;
            self.pixels[row.clone()].copy_from_slice(&background[row]);
        }
    }

    fn clip(&self, area: Rect) -> Option<Rect> {
        let x = area.x.max(0);
        let y = area.y.max(0);
        let right = (area.x + area.width as i32).min(self.width as i32);
        let bottom = (area.y + area.height as i32).min(self.height as i32);
        (x < right && y < bottom).then(|| Rect::new(x, y, (right - x) as u32, (bottom - y) as u32))
    }

    fn outside_labels(
        &mut self,
        font: &Font,
        keys: &KeyMap,
        division: Division,
        area: Rect,
        font_size: f32,
        scale: u32,
    ) {
        let (columns, rows) = division.dimensions();
        let (center_x, _) = area.center();
        let padding = 3 * scale as i32;
        let spacing = (font_size.ceil() as i32 + padding)
            .min(self.width as i32 / columns as i32)
            .min(self.height as i32 / rows as i32)
            .max(1);
        let font_size = font_size.min(spacing as f32);
        let half = spacing / 2;
        let span = spacing * columns as i32;
        let left = (center_x - span / 2).clamp(0, (self.width as i32 - span).max(0));
        let above = area.y - half - padding;
        let below = area.y + area.height as i32 + half + padding;
        let bottom = (self.height as i32 - half).max(half);
        let (first_row, second_row) = match rows {
            2 if above >= half && below <= bottom => (above, below),
            2 if below + spacing <= bottom => (below, below + spacing),
            2 if above - spacing >= half => (above - spacing, above),
            2 => {
                let top = above.clamp(half, (bottom - spacing).max(half));
                (top, top + spacing)
            }
            _ if below <= bottom => (below, below),
            _ => (above, above),
        };

        for index in 0..division.slots() {
            let (column, row) = division.cell(index);
            if let Some(key) = keys.area_key(index) {
                let x = left + column as i32 * spacing + half;
                let y = [first_row, second_row][row].clamp(half, bottom);
                self.label(font, key, (x, y), font_size);
            }
        }
    }

    fn fill(&mut self, area: Rect, color: [u8; 4]) {
        let Some(area) = self.clip(area) else {
            return;
        };
        let stride = self.width as usize * 4;
        let start = area.y as usize * stride + area.x as usize * 4;
        let row = start..start + area.width as usize * 4;
        for pixel in self.pixels[row.clone()].chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
        for y in 1..area.height as usize {
            self.pixels.copy_within(row.clone(), start + y * stride);
        }
    }

    fn outline(&mut self, area: Rect, thickness: u32, color: [u8; 4]) {
        let Rect {
            x,
            y,
            width,
            height,
        } = area;
        let thickness = thickness.min(width).min(height);
        let right = x + (width - thickness) as i32;
        let bottom = y + (height - thickness) as i32;
        for edge in [
            Rect::new(x, y, width, thickness),
            Rect::new(x, bottom, width, thickness),
            Rect::new(x, y, thickness, height),
            Rect::new(right, y, thickness, height),
        ] {
            self.fill(edge, color);
        }
    }

    fn label(&mut self, font: &Font, key: char, (x, y): (i32, i32), size: f32) {
        let (metrics, bitmap) = font.rasterize(key, size);
        self.draw_glyph(&metrics, &bitmap, (x, y));
    }

    fn draw_glyph(&mut self, metrics: &Metrics, bitmap: &[u8], (x, y): (i32, i32)) {
        let left = x - metrics.width as i32 / 2;
        let top = y - metrics.height as i32 / 2;
        let x0 = left.max(0);
        let x1 = (left + metrics.width as i32).min(self.width as i32);
        let y0 = top.max(0);
        let y1 = (top + metrics.height as i32).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let width = (x1 - x0) as usize;
        let opaque = argb(255, LABEL);
        for y in y0..y1 {
            let source = (y - top) as usize * metrics.width + (x0 - left) as usize;
            let start = (y as usize * self.width as usize + x0 as usize) * 4;
            let output = &mut self.pixels[start..start + width * 4];
            for (&alpha, pixel) in bitmap[source..source + width]
                .iter()
                .zip(output.chunks_exact_mut(4))
            {
                match alpha {
                    0 => {}
                    255 => pixel.copy_from_slice(&opaque),
                    _ => {
                        let remaining = u16::from(255 - alpha);
                        for (channel, value) in pixel.iter_mut().zip(argb(alpha, LABEL)) {
                            *channel = value + (u16::from(*channel) * remaining / 255) as u8;
                        }
                    }
                }
            }
        }
    }
}

fn scaled(area: Rect, scale: u32) -> Rect {
    Rect::new(
        area.x * scale as i32,
        area.y * scale as i32,
        area.width * scale,
        area.height * scale,
    )
}

fn argb(alpha: u8, [red, green, blue]: [u8; 3]) -> [u8; 4] {
    let premultiply = |color| (u16::from(color) * u16::from(alpha) / 255) as u8;
    u32::from_be_bytes([
        alpha,
        premultiply(red),
        premultiply(green),
        premultiply(blue),
    ])
    .to_ne_bytes()
}

fn load_font(family: &str) -> Option<Font> {
    let mut database = Database::new();
    database.load_system_fonts();
    let id = database
        .query(&Query {
            families: &[Family::Name(family), Family::SansSerif, Family::Monospace],
            ..Query::default()
        })
        .or_else(|| database.faces().next().map(|face| face.id))?;

    database.with_face_data(id, |data, collection_index| {
        Font::from_bytes(
            data,
            FontSettings {
                collection_index,
                // Fontdue flattens curves when loading the face. Keep enough detail for 4× buffers.
                scale: MAX_CELL_FONT_SIZE * 4.0,
                load_substitutions: false,
            },
        )
        .ok()
    })?
}

#[cfg(test)]
mod tests {
    use super::{BisectState, GridStyle, KeyMap, Overlay, Rect};

    #[test]
    fn moving_the_crosshair_restores_the_background_and_limits_damage() {
        let mut overlay = Overlay {
            style: GridStyle {
                font: None,
                font_size: 20,
                opacity: 102,
            },
            background: Vec::new(),
            previous_frame: None,
        };
        let mut selection = BisectState::new(Rect::new(0, 0, 33, 20), 1.8);
        let keys = KeyMap::default();
        let size = (66, 40);
        let mut buffer = vec![0; 66 * 40 * 4];
        let mut new_buffer = buffer.clone();

        assert_eq!(
            overlay.draw(&mut buffer, size, 2, &selection, &keys, false),
            None
        );
        selection.nudge(-100, -100);
        assert_eq!(
            overlay.draw(&mut buffer, size, 2, &selection, &keys, true),
            Some([Rect::new(24, 12, 18, 18), Rect::new(-8, -8, 18, 18)])
        );
        overlay.draw(&mut new_buffer, size, 2, &selection, &keys, false);
        assert_eq!(buffer, new_buffer);

        selection.nudge(1, 0);
        overlay.draw(&mut new_buffer, size, 2, &selection, &keys, true);
        overlay.draw(&mut buffer, size, 2, &selection, &keys, false);
        assert_eq!(new_buffer, buffer);

        selection.select(0).unwrap();
        assert_eq!(
            overlay.draw(&mut buffer, size, 2, &selection, &keys, true),
            None
        );
    }
}
