use thiserror::Error;

pub const DEFAULT_HOME_ROW_KEYS: &str = "asdfqweruio";
pub const AREA_KEY_COUNT: usize = 8;
pub const CLICK_KEY_COUNT: usize = 3;
pub const REQUIRED_KEY_COUNT: usize = AREA_KEY_COUNT + CLICK_KEY_COUNT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputAction {
    SelectArea(usize),
    Click(MouseButton),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyMap {
    keys: Vec<char>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum KeyMapError {
    #[error("home_row_keys must contain exactly {REQUIRED_KEY_COUNT} characters")]
    WrongLength,
    #[error("home_row_keys must contain unique characters")]
    DuplicateKey,
    #[error("home_row_keys must contain printable ASCII characters")]
    NonPrintable,
}

impl KeyMap {
    pub fn parse(value: &str) -> Result<Self, KeyMapError> {
        let keys: Vec<char> = value.chars().collect();
        if keys.len() != REQUIRED_KEY_COUNT {
            return Err(KeyMapError::WrongLength);
        }
        if keys.iter().any(|key| !key.is_ascii_graphic()) {
            return Err(KeyMapError::NonPrintable);
        }
        for (index, key) in keys.iter().enumerate() {
            if keys[index + 1..].contains(key) {
                return Err(KeyMapError::DuplicateKey);
            }
        }
        Ok(Self { keys })
    }

    pub fn area_key(&self, index: usize) -> Option<char> {
        self.keys
            .get(index)
            .copied()
            .filter(|_| index < AREA_KEY_COUNT)
    }

    pub fn click_key(&self, button: MouseButton) -> char {
        let index = match button {
            MouseButton::Left => AREA_KEY_COUNT,
            MouseButton::Right => AREA_KEY_COUNT + 1,
            MouseButton::Middle => AREA_KEY_COUNT + 2,
        };
        self.keys[index]
    }

    pub fn action_for(&self, key: char) -> InputAction {
        if let Some(index) = self.keys[..AREA_KEY_COUNT]
            .iter()
            .position(|candidate| *candidate == key)
        {
            return InputAction::SelectArea(index);
        }

        let click_index = self.keys[AREA_KEY_COUNT..]
            .iter()
            .position(|candidate| *candidate == key);
        match click_index {
            Some(0) => InputAction::Click(MouseButton::Left),
            Some(1) => InputAction::Click(MouseButton::Right),
            Some(2) => InputAction::Click(MouseButton::Middle),
            _ => InputAction::Unknown,
        }
    }

    pub fn as_str(&self) -> String {
        self.keys.iter().collect()
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::parse(DEFAULT_HOME_ROW_KEYS).expect("the default key map is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::{InputAction, KeyMap, KeyMapError, MouseButton};

    #[test]
    fn default_map_assigns_the_last_three_keys_to_mouse_buttons() {
        let map = KeyMap::default();

        assert_eq!(map.action_for('u'), InputAction::Click(MouseButton::Left));
        assert_eq!(map.action_for('i'), InputAction::Click(MouseButton::Right));
        assert_eq!(map.action_for('o'), InputAction::Click(MouseButton::Middle));
    }

    #[test]
    fn the_first_eight_keys_select_areas() {
        let map = KeyMap::default();

        assert_eq!(map.action_for('a'), InputAction::SelectArea(0));
        assert_eq!(map.action_for('r'), InputAction::SelectArea(7));
        assert_eq!(map.action_for('x'), InputAction::Unknown);
    }

    #[test]
    fn invalid_maps_are_rejected() {
        assert_eq!(KeyMap::parse("asdf").unwrap_err(), KeyMapError::WrongLength);
        assert_eq!(
            KeyMap::parse("asdfqweruuo").unwrap_err(),
            KeyMapError::DuplicateKey
        );
    }
}
