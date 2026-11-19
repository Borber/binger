use thiserror::Error;

pub const MAX_HISTORY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn center(self) -> (i32, i32) {
        (
            self.x + (self.width / 2) as i32,
            self.y + (self.height / 2) as i32,
        )
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Division {
    Grid8,
    Grid4,
    Horizontal,
    Vertical,
    Undividable,
}

impl Division {
    pub(crate) const fn dimensions(self) -> (usize, usize) {
        match self {
            Self::Grid8 => (4, 2),
            Self::Grid4 => (2, 2),
            Self::Horizontal => (2, 1),
            Self::Vertical => (1, 2),
            Self::Undividable => (0, 0),
        }
    }

    pub const fn slots(self) -> usize {
        let (columns, rows) = self.dimensions();
        columns * rows
    }

    pub(crate) fn cell(self, index: usize) -> (usize, usize) {
        let (columns, _) = self.dimensions();
        let row = index / columns;
        // The first four keys stay available in smaller grids. In 4×2
        // grids, the next four form the top row.
        let row = if self == Self::Grid8 { 1 - row } else { row };
        (index % columns, row)
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SelectError {
    #[error("the selected area index is outside the current division")]
    InvalidIndex,
    #[error("the current area cannot be divided further")]
    Undividable,
    #[error("the selection history reached its maximum depth")]
    HistoryLimit,
}

#[derive(Clone, Debug)]
pub struct BisectState {
    bounds: Rect,
    current: Rect,
    target: (i32, i32),
    history: Vec<Rect>,
    ratio: f32,
}

impl BisectState {
    pub fn new(initial: Rect, ratio: f32) -> Self {
        Self {
            bounds: initial,
            current: initial,
            target: initial.center(),
            history: Vec::new(),
            ratio,
        }
    }

    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    pub const fn current(&self) -> Rect {
        self.current
    }

    pub const fn target(&self) -> (i32, i32) {
        self.target
    }

    pub fn nudge(&mut self, dx: i32, dy: i32) -> bool {
        if self.bounds.is_empty() {
            return false;
        }
        let Rect {
            x,
            y,
            width,
            height,
        } = self.bounds;
        let offset = |position: i32, delta: i32, start: i32, length: u32| {
            position
                .saturating_add(delta)
                .clamp(start, start.saturating_add_unsigned(length - 1))
        };
        let (target_x, target_y) = self.target;
        let target = (
            offset(target_x, dx, x, width),
            offset(target_y, dy, y, height),
        );
        let moved = target != self.target;
        self.target = target;
        moved
    }

    pub fn division(&self) -> Division {
        determine_division(self.current, self.ratio)
    }

    /// Returns the rectangles in the current division in keyboard-label order.
    pub fn subareas(&self) -> Vec<Rect> {
        let division = self.division();
        (0..division.slots())
            .map(|index| self.subarea_for(division, index))
            .collect()
    }

    pub(crate) fn subarea_for(&self, division: Division, index: usize) -> Rect {
        sub_rect(self.current, division, index)
    }

    pub fn depth(&self) -> usize {
        self.history.len()
    }

    pub fn select(&mut self, index: usize) -> Result<Rect, SelectError> {
        let division = self.division();
        if division == Division::Undividable {
            return Err(SelectError::Undividable);
        }
        if index >= division.slots() {
            return Err(SelectError::InvalidIndex);
        }
        if self.history.len() >= MAX_HISTORY {
            return Err(SelectError::HistoryLimit);
        }

        let next = sub_rect(self.current, division, index);
        self.history.push(self.current);
        self.current = next;
        self.target = next.center();
        Ok(next)
    }

    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(previous) => {
                self.current = previous;
                self.target = previous.center();
                true
            }
            None => false,
        }
    }
}

fn determine_division(area: Rect, ratio: f32) -> Division {
    if area.is_empty() || (area.width == 1 && area.height == 1) {
        return Division::Undividable;
    }
    if area.width <= 1 {
        return Division::Vertical;
    }
    if area.height <= 1 {
        return Division::Horizontal;
    }
    if area.width >= 4 && (area.width as f32) > area.height as f32 * ratio {
        Division::Grid8
    } else {
        Division::Grid4
    }
}

fn segment(total: u32, parts: usize, index: usize) -> (u32, u32) {
    let parts = parts as u32;
    let index = index as u32;
    let base = total / parts;
    let remainder = total % parts;
    let offset = index * base + index.min(remainder);
    let length = base + u32::from(index < remainder);
    (offset, length)
}

fn sub_rect(area: Rect, division: Division, index: usize) -> Rect {
    let (columns, rows) = division.dimensions();
    let (column, row) = division.cell(index);
    let (x_offset, width) = segment(area.width, columns, column);
    let (y_offset, height) = segment(area.height, rows, row);
    Rect::new(
        area.x + x_offset as i32,
        area.y + y_offset as i32,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::{BisectState, Division, Rect, SelectError};

    #[test]
    fn wide_areas_use_eight_cells() {
        let state = BisectState::new(Rect::new(0, 0, 2000, 1000), 1.8);
        assert_eq!(state.division(), Division::Grid8);
    }

    #[test]
    fn square_areas_use_four_cells() {
        let state = BisectState::new(Rect::new(0, 0, 1000, 1000), 1.8);
        assert_eq!(state.division(), Division::Grid4);
    }

    #[test]
    fn remainder_pixels_are_distributed_without_gaps() {
        let mut state = BisectState::new(Rect::new(10, 20, 101, 51), 1.8);
        let areas = state.subareas();
        assert_eq!(state.select(0).unwrap(), Rect::new(10, 46, 26, 25));
        assert!(state.undo());
        assert_eq!(state.select(4).unwrap(), Rect::new(10, 20, 26, 26));
        assert_eq!(areas[4], state.current());
        assert!(state.undo());
        assert_eq!(state.select(7).unwrap(), Rect::new(86, 20, 25, 26));
    }

    #[test]
    fn backspace_restores_the_previous_area() {
        let initial = Rect::new(0, 0, 1920, 1080);
        let mut state = BisectState::new(initial, 1.8);
        let selected = state.select(3).unwrap();
        assert_eq!(state.depth(), 1);
        assert!(state.undo());
        assert_eq!(state.current(), initial);
        assert_ne!(selected, initial);
        assert!(!state.undo());
    }

    #[test]
    fn nudges_can_leave_the_selected_area_and_stop_at_the_output_edges() {
        let mut state = BisectState::new(Rect::new(10, 20, 4, 4), 1.8);
        let selected = state.select(0).unwrap();
        assert_eq!(state.target(), (11, 21));

        assert!(state.nudge(1, 0));
        assert!(state.nudge(1, 0));
        assert!(!state.nudge(1, 0));
        assert_eq!(state.target(), (13, 21));
        assert_eq!(state.current(), selected);
        assert_eq!(state.depth(), 1);

        assert!(state.nudge(-20, -20));
        assert_eq!(state.target(), (10, 20));
        assert!(state.undo());
        assert_eq!(state.target(), (12, 22));
    }

    #[test]
    fn invalid_and_undividable_selections_are_reported() {
        let mut state = BisectState::new(Rect::new(0, 0, 10, 10), 1.8);
        assert_eq!(state.select(4).unwrap_err(), SelectError::InvalidIndex);

        let mut pixel = BisectState::new(Rect::new(0, 0, 1, 1), 1.8);
        assert_eq!(pixel.select(0).unwrap_err(), SelectError::Undividable);

        let narrow = BisectState::new(Rect::new(0, 0, 3, 2), 0.5);
        assert!(narrow.subareas().iter().all(|area| !area.is_empty()));
    }
}
