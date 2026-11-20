use std::{ffi::OsStr, fs, ops::Range, os::unix::ffi::OsStrExt, process::Command};

use ab_glyph::{Font, FontVec};
use fontdb::{Database, Family, Query};

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
    crosshair_background: Vec<u8>,
    previous_frame: Option<(GridLayout, Rect)>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct GridLayout {
    size: (u32, u32),
    scale: u32,
    area: Rect,
}

struct GridStyle {
    font: Option<FontVec>,
    font_size: u32,
    opacity: u8,
}

impl Overlay {
    pub(super) fn new(config: &Config, font: Option<FontVec>) -> Self {
        Self {
            style: GridStyle {
                font,
                font_size: config.label_font_size.max(1),
                opacity: (config.overlay_opacity * 255.0).round() as u8,
            },
            background: Vec::new(),
            crosshair_background: Vec::new(),
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
        let mut canvas = Canvas {
            pixels,
            width,
            height,
        };
        match previous_crosshair {
            Some(previous) if reuse_buffer => {
                let mut saved = self.crosshair_background.as_slice();
                for row in canvas.rows(previous) {
                    let (pixels, rest) = saved.split_at(row.len());
                    canvas.pixels[row].copy_from_slice(pixels);
                    saved = rest;
                }
            }
            Some(_) => {
                // A second buffer may be needed while the compositor holds the previous one.
                if self.background.is_empty() {
                    self.background.resize(canvas.pixels.len(), 0);
                    let mut background = Canvas {
                        pixels: &mut self.background,
                        width,
                        height,
                    };
                    self.style.draw(&mut background, selection, keys, scale);
                }
                canvas.pixels.copy_from_slice(&self.background);
            }
            None => {
                self.background.clear();
                if scaled(layout.area, scale) != Rect::new(0, 0, width, height)
                    || selection.division() == Division::Undividable
                {
                    canvas.pixels.fill(0);
                }
                self.style.draw(&mut canvas, selection, keys, scale);
            }
        }

        let (x, y) = selection.target();
        let arm = 4 * scale;
        let x = x * scale as i32;
        let y = y * scale as i32;
        let length = 2 * arm + scale;
        let left = x - arm as i32;
        let top = y - arm as i32;
        let crosshair = Rect::new(left, top, length, length);
        self.crosshair_background.clear();
        for row in canvas.rows(crosshair) {
            self.crosshair_background
                .extend_from_slice(&canvas.pixels[row]);
        }
        let color = argb(230, CROSSHAIR);
        canvas.fill(Rect::new(left, y, length, scale), color);
        canvas.fill(Rect::new(x, top, scale, length), color);
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
    fn rows(&self, area: Rect) -> impl Iterator<Item = Range<usize>> + use<> {
        let area = self.clip(area).unwrap_or(Rect::new(0, 0, 0, 0));
        let stride = self.width as usize * 4;
        let start = area.y as usize * stride + area.x as usize * 4;
        (0..area.height as usize).map(move |y| {
            let start = start + y * stride;
            start..start + area.width as usize * 4
        })
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
        font: &FontVec,
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

    fn label(&mut self, font: &FontVec, key: char, (x, y): (i32, i32), size: f32) {
        let Some(units_per_em) = font.units_per_em() else {
            return;
        };
        // Label sizes use pixels per em. ab_glyph scales by line height.
        let glyph = font
            .glyph_id(key)
            .with_scale(size * font.height_unscaled() / units_per_em);
        let Some(outline) = font.outline_glyph(glyph) else {
            return;
        };
        let bounds = outline.px_bounds();
        let left = x - bounds.width() as i32 / 2;
        let top = y - bounds.height() as i32 / 2;

        let opaque = argb(255, LABEL);
        outline.draw(|x, y, coverage| {
            let alpha = (coverage * 255.0 + 0.5) as u8;
            let x = (left + x as i32) as u32;
            let y = (top + y as i32) as u32;
            if alpha == 0 || x >= self.width || y >= self.height {
                return;
            }
            let start = (y as usize * self.width as usize + x as usize) * 4;
            let pixel = &mut self.pixels[start..start + 4];
            if alpha == 255 {
                pixel.copy_from_slice(&opaque);
            } else {
                let remaining = u16::from(255 - alpha);
                for (channel, value) in pixel.iter_mut().zip(argb(alpha, LABEL)) {
                    *channel = value + (u16::from(*channel) * remaining / 255) as u8;
                }
            }
        });
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

pub(super) fn load_font(family: &str) -> Option<FontVec> {
    if let Some(font) = load_fontconfig_match(family) {
        return Some(font);
    }

    let mut database = Database::new();
    database.load_system_fonts();
    let id = database
        .query(&Query {
            families: &[Family::Name(family), Family::SansSerif, Family::Monospace],
            ..Query::default()
        })
        .or_else(|| database.faces().next().map(|face| face.id))?;

    database.with_face_data(id, |data, collection_index| {
        FontVec::try_from_vec_and_index(data.to_vec(), collection_index).ok()
    })?
}

fn load_fontconfig_match(family: &str) -> Option<FontVec> {
    let mut pattern = String::from(":family=");
    for character in family.chars() {
        if matches!(character, '\\' | ',' | ':' | '=') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    let output = Command::new("fc-match")
        .args(["--format", "%{index}\\n%{file}"])
        .arg(pattern)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let mut fields = output.stdout.splitn(2, |&byte| byte == b'\n');
    let index = std::str::from_utf8(fields.next()?).ok()?;
    let path = OsStr::from_bytes(fields.next()?);
    // Fontconfig stores a variable font's named instance in the high 16 bits.
    let face_index = index.parse::<u32>().ok()? & 0xffff;
    FontVec::try_from_vec_and_index(fs::read(path).ok()?, face_index).ok()
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
            crosshair_background: Vec::new(),
            previous_frame: None,
        };
        let mut selection = BisectState::new(Rect::new(0, 0, 33, 20), 1.8);
        let keys = KeyMap::default();
        let size = (66, 40);
        let mut buffer = vec![255; 66 * 40 * 4];
        let mut new_buffer = vec![0; buffer.len()];

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
        assert_eq!(&buffer[buffer.len() - 4..], &[0; 4]);
    }
}
