use fontdb::{Database, Family, Query};
use fontdue::{Font, FontSettings};

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
    font: Option<Font>,
    font_size: u32,
    opacity: u8,
}

impl Overlay {
    pub(super) fn new(config: &Config) -> Self {
        Self {
            font: load_font(&config.label_font_family),
            font_size: config.label_font_size.max(1),
            opacity: (config.overlay_opacity * 255.0).round() as u8,
        }
    }

    pub(super) fn draw(
        &self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        scale: u32,
        selection: &BisectState,
        keys: &KeyMap,
    ) {
        pixels.fill(0);
        let mut canvas = Canvas {
            pixels,
            width,
            height,
        };
        let selected = scaled(selection.current(), scale);
        let division = selection.division();
        let cells = selection.subareas();
        let (center_x, center_y) = selection.current().center();
        let center_x = center_x * scale as i32;
        let center_y = center_y * scale as i32;
        let font_size = self.font_size.min(width.min(height) / scale).max(1) as f32 * scale as f32;

        let outside = cells.iter().any(|cell| {
            let height = (cell.height * scale) as f32;
            let width = (cell.width * scale) as f32;
            height <= font_size || width <= font_size
        });
        for (index, cell) in cells.iter().enumerate() {
            let (column, row) = division.cell(index);
            let (fill, edge) = if (column + row) % 2 == 0 {
                (TEAL, TEAL_EDGE)
            } else {
                (PLUM, PLUM_EDGE)
            };
            let area = scaled(*cell, scale);
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

        let arm = 4 * scale;
        let color = argb(230, CROSSHAIR);
        canvas.fill(
            Rect::new(center_x - arm as i32, center_y, 2 * arm + scale, scale),
            color,
        );
        canvas.fill(
            Rect::new(center_x, center_y - arm as i32, scale, 2 * arm + scale),
            color,
        );
    }
}

struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: u32,
    height: u32,
}

impl Canvas<'_> {
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
        let width = self.width as i32;
        let x0 = area.x.clamp(0, width) as usize;
        let x1 = (area.x + area.width as i32).clamp(0, width) as usize;
        let y0 = area.y.max(0);
        let y1 = (area.y + area.height as i32).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = self.width as usize * 4;
        let start = y0 as usize * stride + x0 * 4;
        let row = start..start + (x1 - x0) * 4;
        for pixel in self.pixels[row.clone()].chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
        for y in 1..(y1 - y0) as usize {
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
        let left = x - metrics.width as i32 / 2;
        let top = y - metrics.height as i32 / 2;
        for (index, &alpha) in bitmap.iter().enumerate() {
            if alpha != 0 {
                let x = left + (index % metrics.width) as i32;
                let y = top + (index / metrics.width) as i32;
                self.blend(x, y, LABEL, alpha);
            }
        }
    }

    fn blend(&mut self, x: i32, y: i32, rgb: [u8; 3], alpha: u8) {
        if x < 0 || x >= self.width as i32 || y < 0 || y >= self.height as i32 {
            return;
        }
        let index = (y as usize * self.width as usize + x as usize) * 4;
        let remaining = u16::from(255 - alpha);
        for (channel, value) in self.pixels[index..index + 4]
            .iter_mut()
            .zip(argb(alpha, rgb))
        {
            *channel = value + (u16::from(*channel) * remaining / 255) as u8;
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
