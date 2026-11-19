use std::{fs, path::Path};

use serde::Deserialize;
use thiserror::Error;

use crate::input::{DEFAULT_HOME_ROW_KEYS, KeyMap, KeyMapError};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub home_row_keys: String,
    pub bisect_ratio: f32,
    pub label_font_family: String,
    pub label_font_size: u32,
    pub overlay_opacity: f32,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read config {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("could not parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid key map: {0}")]
    KeyMap(#[from] KeyMapError),
    #[error("bisect_ratio must be greater than zero")]
    InvalidRatio,
    #[error("overlay_opacity must be between 0 and 1")]
    InvalidOpacity,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            home_row_keys: DEFAULT_HOME_ROW_KEYS.to_owned(),
            bisect_ratio: 1.8,
            label_font_family: "Maple Mono NF".to_owned(),
            label_font_size: 20,
            overlay_opacity: 0.4,
        }
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let config: Self = toml::from_str(&text)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        KeyMap::parse(&self.home_row_keys)?;
        if self.bisect_ratio <= 0.0 {
            return Err(ConfigError::InvalidRatio);
        }
        if !(0.0..=1.0).contains(&self.overlay_opacity) {
            return Err(ConfigError::InvalidOpacity);
        }
        Ok(())
    }

    pub fn key_map(&self) -> Result<KeyMap, ConfigError> {
        Ok(KeyMap::parse(&self.home_row_keys)?)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn defaults_match_the_documented_settings() {
        let config = Config::default();
        assert_eq!(config.home_row_keys, "asdfqweruio");
        assert_eq!(config.bisect_ratio, 1.8);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn toml_can_override_the_key_map() {
        let config: Config = toml::from_str(
            r#"
home_row_keys = "qwertyuiopz"
bisect_ratio = 2.0
"#,
        )
        .unwrap();
        assert_eq!(config.home_row_keys, "qwertyuiopz");
        assert_eq!(config.bisect_ratio, 2.0);
    }
}
