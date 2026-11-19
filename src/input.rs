use thiserror::Error;

pub const DEFAULT_HOME_ROW_KEYS: &str = "asdfqweruio";
pub const DEFAULT_NUDGE_KEYS: &str = "hjkl";
pub const AREA_KEY_COUNT: usize = 8;
pub const CLICK_KEY_COUNT: usize = 3;
pub const REQUIRED_KEY_COUNT: usize = AREA_KEY_COUNT + CLICK_KEY_COUNT;
const NUDGE_DIRECTIONS: [(i32, i32); 4] = [(-1, 0), (0, 1), (0, -1), (1, 0)];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    const ALL: [Self; CLICK_KEY_COUNT] = [Self::Left, Self::Right, Self::Middle];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputAction {
    SelectArea(usize),
    Click(MouseButton),
    Nudge { dx: i32, dy: i32 },
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyMap {
    keys: Vec<char>,
    nudge_keys: Vec<char>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum KeyMapError {
    #[error("home_row_keys must contain exactly {REQUIRED_KEY_COUNT} characters")]
    WrongLength,
    #[error("home_row_keys must contain unique characters")]
    DuplicateKey,
    #[error("home_row_keys must contain printable ASCII characters")]
    NonPrintable,
    #[error("nudge_keys must contain exactly four characters")]
    NudgeWrongLength,
    #[error("nudge_keys must contain unique characters")]
    NudgeDuplicateKey,
    #[error("nudge_keys must contain printable ASCII characters")]
    NudgeNonPrintable,
}

impl KeyMap {
    pub fn parse(value: &str) -> Result<Self, KeyMapError> {
        Self::parse_with_nudge_keys(value, DEFAULT_NUDGE_KEYS)
    }

    pub fn parse_with_nudge_keys(value: &str, nudge_keys: &str) -> Result<Self, KeyMapError> {
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
        let nudge_keys: Vec<char> = nudge_keys.chars().collect();
        if nudge_keys.len() != NUDGE_DIRECTIONS.len() {
            return Err(KeyMapError::NudgeWrongLength);
        }
        if nudge_keys.iter().any(|key| !key.is_ascii_graphic()) {
            return Err(KeyMapError::NudgeNonPrintable);
        }
        for (index, key) in nudge_keys.iter().enumerate() {
            if nudge_keys[index + 1..].contains(key) {
                return Err(KeyMapError::NudgeDuplicateKey);
            }
        }
        Ok(Self { keys, nudge_keys })
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
        let lower = key.to_ascii_lowercase();
        let position = |key| self.keys.iter().position(|&candidate| candidate == key);
        let index = position(key).or_else(|| if key == lower { None } else { position(lower) });
        match index {
            Some(index) if index < AREA_KEY_COUNT => InputAction::SelectArea(index),
            Some(index) => InputAction::Click(MouseButton::ALL[index - AREA_KEY_COUNT]),
            None => {
                if let Some((index, _)) = self
                    .nudge_keys
                    .iter()
                    .enumerate()
                    .find(|(_, key)| key.eq_ignore_ascii_case(&lower))
                {
                    let (dx, dy) = NUDGE_DIRECTIONS[index];
                    InputAction::Nudge { dx, dy }
                } else {
                    InputAction::Unknown
                }
            }
        }
    }

    pub fn nudge_keys(&self) -> impl Iterator<Item = char> + '_ {
        self.nudge_keys
            .iter()
            .copied()
            .filter(|&key| matches!(self.action_for(key), InputAction::Nudge { .. }))
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
    fn nudge_keys_respect_explicit_mappings() {
        let map = KeyMap::default();
        assert_eq!(map.nudge_keys().collect::<String>(), "hjkl");
        assert_eq!(map.action_for('h'), InputAction::Nudge { dx: -1, dy: 0 });
        assert_eq!(map.action_for('j'), InputAction::Nudge { dx: 0, dy: 1 });
        assert_eq!(map.action_for('k'), InputAction::Nudge { dx: 0, dy: -1 });
        assert_eq!(map.action_for('L'), InputAction::Nudge { dx: 1, dy: 0 });

        let legacy = KeyMap::parse("asdfqwerjkl").unwrap();
        assert_eq!(legacy.nudge_keys().collect::<String>(), "h");
        assert_eq!(
            legacy.action_for('J'),
            InputAction::Click(MouseButton::Left)
        );

        let custom = KeyMap::parse("hsdfqwerHio").unwrap();
        assert_eq!(custom.nudge_keys().collect::<String>(), "jkl");
        assert_eq!(custom.action_for('h'), InputAction::SelectArea(0));
        assert_eq!(
            custom.action_for('H'),
            InputAction::Click(MouseButton::Left)
        );
    }

    #[test]
    fn custom_nudge_keys_follow_direction_order() {
        let map = KeyMap::parse_with_nudge_keys("asdfqweruio", "zxcv").unwrap();

        assert_eq!(map.nudge_keys().collect::<String>(), "zxcv");
        assert_eq!(map.action_for('z'), InputAction::Nudge { dx: -1, dy: 0 });
        assert_eq!(map.action_for('x'), InputAction::Nudge { dx: 0, dy: 1 });
        assert_eq!(map.action_for('c'), InputAction::Nudge { dx: 0, dy: -1 });
        assert_eq!(map.action_for('v'), InputAction::Nudge { dx: 1, dy: 0 });
    }

    #[test]
    fn invalid_maps_are_rejected() {
        assert_eq!(KeyMap::parse("asdf").unwrap_err(), KeyMapError::WrongLength);
        assert_eq!(
            KeyMap::parse("asdfqweruuo").unwrap_err(),
            KeyMapError::DuplicateKey
        );
        assert_eq!(
            KeyMap::parse_with_nudge_keys("asdfqweruio", "hjk").unwrap_err(),
            KeyMapError::NudgeWrongLength
        );
    }
}
