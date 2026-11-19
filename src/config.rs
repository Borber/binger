use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

use crate::input::{DEFAULT_HOME_ROW_KEYS, DEFAULT_NUDGE_KEYS, KeyMap, KeyMapError};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub home_row_keys: String,
    pub nudge_keys: String,
    pub bisect_ratio: f32,
    pub label_font_family: String,
    pub label_font_size: u32,
    pub overlay_opacity: f32,
    pub nudge_step: i32,
    pub nudge_interval_ms: u64,
    pub nudge_acceleration_interval_ms: u64,
    pub nudge_max_step: i32,
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
    #[error("nudge_step must be greater than zero")]
    InvalidNudgeStep,
    #[error("nudge_interval_ms must be greater than zero")]
    InvalidNudgeInterval,
    #[error("nudge_acceleration_interval_ms must be greater than zero")]
    InvalidNudgeAccelerationInterval,
    #[error("nudge_max_step must be greater than zero")]
    InvalidNudgeMaxStep,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            home_row_keys: DEFAULT_HOME_ROW_KEYS.to_owned(),
            nudge_keys: DEFAULT_NUDGE_KEYS.to_owned(),
            bisect_ratio: 1.8,
            label_font_family: "Maple Mono NF".to_owned(),
            label_font_size: 20,
            overlay_opacity: 0.4,
            nudge_step: 1,
            nudge_interval_ms: 15,
            nudge_acceleration_interval_ms: 160,
            nudge_max_step: 6,
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
        Self::parse(&text)
    }

    pub fn load_default() -> Result<Self, ConfigError> {
        let Some(path) = default_path() else {
            return Ok(Self::default());
        };

        match Self::load(&path) {
            Ok(config) => Ok(config),
            Err(ConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(Self::default())
            }
            Err(error) => Err(error),
        }
    }

    fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        KeyMap::parse_with_nudge_keys(&self.home_row_keys, &self.nudge_keys)?;
        if self.bisect_ratio <= 0.0 {
            return Err(ConfigError::InvalidRatio);
        }
        if !(0.0..=1.0).contains(&self.overlay_opacity) {
            return Err(ConfigError::InvalidOpacity);
        }
        if self.nudge_step <= 0 {
            return Err(ConfigError::InvalidNudgeStep);
        }
        if self.nudge_interval_ms == 0 {
            return Err(ConfigError::InvalidNudgeInterval);
        }
        if self.nudge_acceleration_interval_ms == 0 {
            return Err(ConfigError::InvalidNudgeAccelerationInterval);
        }
        if self.nudge_max_step <= 0 {
            return Err(ConfigError::InvalidNudgeMaxStep);
        }
        Ok(())
    }

    pub fn key_map(&self) -> Result<KeyMap, ConfigError> {
        Ok(KeyMap::parse_with_nudge_keys(
            &self.home_row_keys,
            &self.nudge_keys,
        )?)
    }
}

fn default_path() -> Option<PathBuf> {
    config_base_dir(
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
    .map(|base| base.join("binger/config.toml"))
}

fn config_base_dir(xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    xdg_config_home
        .filter(|path| !path.is_empty() && Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| home.map(|path| PathBuf::from(path).join(".config")))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Config, config_base_dir};

    #[test]
    fn defaults_match_the_documented_settings() {
        let config = Config::default();
        assert_eq!(config.home_row_keys, "asdfqweruio");
        assert_eq!(config.nudge_keys, "hjkl");
        assert_eq!(config.bisect_ratio, 1.8);
        assert_eq!(config.nudge_step, 1);
        assert_eq!(config.nudge_interval_ms, 15);
        assert_eq!(config.nudge_acceleration_interval_ms, 160);
        assert_eq!(config.nudge_max_step, 6);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn toml_can_override_the_key_map() {
        let config: Config = toml::from_str(
            r#"
home_row_keys = "qwertyuiopz"
nudge_keys = "hjkl"
bisect_ratio = 2.0
nudge_step = 2
"#,
        )
        .unwrap();
        assert_eq!(config.home_row_keys, "qwertyuiopz");
        assert_eq!(config.nudge_keys, "hjkl");
        assert_eq!(config.bisect_ratio, 2.0);
        assert_eq!(config.nudge_step, 2);
    }

    #[test]
    fn xdg_config_home_takes_precedence_over_home() {
        let base = config_base_dir(
            Some(Path::new("/custom/config").as_os_str()),
            Some(Path::new("/home/test").as_os_str()),
        )
        .unwrap();
        assert_eq!(base, Path::new("/custom/config"));
    }

    #[test]
    fn home_falls_back_to_dot_config() {
        let base = config_base_dir(None, Some(Path::new("/home/test").as_os_str())).unwrap();
        assert_eq!(base, Path::new("/home/test/.config"));
    }

    #[test]
    fn relative_xdg_config_home_is_ignored() {
        let base = config_base_dir(
            Some(Path::new("relative/config").as_os_str()),
            Some(Path::new("/home/test").as_os_str()),
        )
        .unwrap();
        assert_eq!(base, Path::new("/home/test/.config"));
    }
}
