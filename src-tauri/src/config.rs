use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::error::CommandError;
use crate::storage::write_private_atomically;

const CONFIG_FILE_NAME: &str = "config.json";
const CURRENT_SCHEMA_VERSION: u16 = 1;
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_FONT_FAMILY_CHARACTERS: usize = 256;
const DEFAULT_FONT_FAMILY: &str =
    "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct AppConfig {
    pub schema_version: u16,
    pub appearance: AppearanceConfig,
    pub terminal: TerminalConfig,
    pub vim_mode: bool,
    pub animations: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            appearance: AppearanceConfig::default(),
            terminal: TerminalConfig::default(),
            vim_mode: false,
            animations: true,
        }
    }
}

impl AppConfig {
    fn normalize_and_validate(mut self) -> Result<Self, CommandError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(invalid_config(
                "The configuration schema version is not supported.",
            ));
        }

        self.appearance.validate()?;
        self.terminal.normalize_and_validate()?;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct AppearanceConfig {
    pub mode: AppearanceMode,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl AppearanceConfig {
    fn validate(&self) -> Result<(), CommandError> {
        match (self.latitude, self.longitude) {
            (None, None) => Ok(()),
            (Some(latitude), Some(longitude)) => {
                validate_finite_range("Latitude", latitude, -90.0, 90.0)?;
                validate_finite_range("Longitude", longitude, -180.0, 180.0)
            }
            _ => Err(invalid_config(
                "Latitude and longitude must either both be set or both be omitted.",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
    SunSchedule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub font_family: String,
    pub font_size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub font_weight: u16,
    pub font_ligatures: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: DEFAULT_FONT_FAMILY.to_owned(),
            font_size: 14.0,
            line_height: 1.18,
            letter_spacing: 0.0,
            font_weight: 400,
            font_ligatures: true,
        }
    }
}

impl TerminalConfig {
    fn normalize_and_validate(&mut self) -> Result<(), CommandError> {
        if self.font_family.chars().any(char::is_control) {
            return Err(invalid_config(
                "The terminal font family must not contain control characters.",
            ));
        }

        self.font_family = self.font_family.trim().to_owned();
        if self.font_family.is_empty() {
            return Err(invalid_config(
                "The terminal font family must not be empty.",
            ));
        }
        if self.font_family.chars().count() > MAX_FONT_FAMILY_CHARACTERS {
            return Err(invalid_config(
                "The terminal font family must not exceed 256 characters.",
            ));
        }

        validate_finite_range("Font size", self.font_size, 8.0, 40.0)?;
        validate_finite_range("Line height", self.line_height, 1.0, 2.0)?;
        validate_finite_range("Letter spacing", self.letter_spacing, -2.0, 5.0)?;
        if !(100..=900).contains(&self.font_weight) {
            return Err(invalid_config("Font weight must be between 100 and 900."));
        }

        Ok(())
    }
}

#[tauri::command(async)]
pub fn config_load(app: AppHandle) -> Result<AppConfig, CommandError> {
    load_from_path(&config_path(&app)?)
}

#[tauri::command(async)]
pub fn config_save(app: AppHandle, config: AppConfig) -> Result<AppConfig, CommandError> {
    save_to_path(&config_path(&app)?, config)
}

fn config_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(CONFIG_FILE_NAME))
        .map_err(|_| {
            CommandError::new(
                "config_path_unavailable",
                "The application configuration directory is unavailable.",
            )
        })
}

fn load_from_path(path: &Path) -> Result<AppConfig, CommandError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppConfig::default())
        }
        Err(_) => {
            return Err(CommandError::new(
                "config_read_failed",
                "The application configuration could not be read.",
            ))
        }
    };

    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            CommandError::new(
                "config_read_failed",
                "The application configuration could not be read.",
            )
        })?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(CommandError::new(
            "config_too_large",
            "The application configuration exceeds the 64 KiB limit.",
        ));
    }

    serde_json::from_slice::<AppConfig>(&bytes)
        .map_err(|_| {
            CommandError::new(
                "config_parse_failed",
                "The application configuration is malformed or uses unsupported fields.",
            )
        })?
        .normalize_and_validate()
}

fn save_to_path(path: &Path, config: AppConfig) -> Result<AppConfig, CommandError> {
    let config = config.normalize_and_validate()?;
    let mut bytes = serde_json::to_vec_pretty(&config).map_err(|_| config_write_error())?;
    bytes.push(b'\n');
    write_private_atomically(path, &bytes).map_err(|_| config_write_error())?;
    Ok(config)
}

fn validate_finite_range(
    field_name: &str,
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), CommandError> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(invalid_config(format!(
            "{field_name} must be a finite number between {minimum} and {maximum}."
        )));
    }
    Ok(())
}

fn invalid_config(message: impl Into<String>) -> CommandError {
    CommandError::new("config_validation_failed", message)
}

fn config_write_error() -> CommandError {
    CommandError::new(
        "config_write_failed",
        "The application configuration could not be saved.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("twominal-config-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create test directory");
            Self { path }
        }

        fn config_path(&self) -> PathBuf {
            self.path.join(CONFIG_FILE_NAME)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn defaults_are_stable_and_use_the_frontend_schema() {
        let config = AppConfig::default();
        assert_eq!(config.schema_version, 1);
        assert_eq!(config.appearance.mode, AppearanceMode::System);
        assert_eq!(config.appearance.latitude, None);
        assert_eq!(config.appearance.longitude, None);
        assert_eq!(config.terminal.font_family, DEFAULT_FONT_FAMILY);
        assert_eq!(config.terminal.font_size, 14.0);
        assert_eq!(config.terminal.line_height, 1.18);
        assert_eq!(config.terminal.letter_spacing, 0.0);
        assert_eq!(config.terminal.font_weight, 400);
        assert!(config.terminal.font_ligatures);
        assert!(!config.vim_mode);
        assert!(config.animations);

        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["appearance"]["mode"], "system");
        assert_eq!(value["terminal"]["fontFamily"], DEFAULT_FONT_FAMILY);
        assert_eq!(value["terminal"]["fontLigatures"], true);
        assert_eq!(value["vimMode"], false);
    }

    #[test]
    fn missing_fields_migrate_to_current_defaults() {
        let empty: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, AppConfig::default());

        let partial: AppConfig =
            serde_json::from_str(r#"{"appearance":{"mode":"dark"},"terminal":{"fontSize":16.0}}"#)
                .unwrap();
        assert_eq!(partial.schema_version, 1);
        assert_eq!(partial.appearance.mode, AppearanceMode::Dark);
        assert_eq!(partial.terminal.font_size, 16.0);
        assert_eq!(partial.terminal.font_family, DEFAULT_FONT_FAMILY);
        assert!(!partial.vim_mode);
        assert!(partial.animations);
    }

    #[test]
    fn validation_normalizes_font_family_and_accepts_boundaries() {
        let mut config = AppConfig::default();
        config.terminal.font_family = "  JetBrains Mono  ".to_owned();
        config.terminal.font_size = 8.0;
        config.terminal.line_height = 2.0;
        config.terminal.letter_spacing = -2.0;
        config.terminal.font_weight = 900;
        config.appearance.latitude = Some(-90.0);
        config.appearance.longitude = Some(180.0);

        let validated = config.normalize_and_validate().unwrap();
        assert_eq!(validated.terminal.font_family, "JetBrains Mono");
    }

    #[test]
    fn validation_rejects_invalid_schema_and_font_values() {
        let config = AppConfig {
            schema_version: 2,
            ..AppConfig::default()
        };
        assert_validation_error(config);

        for family in [String::new(), "Mono\nInjected".to_owned(), "x".repeat(257)] {
            let mut config = AppConfig::default();
            config.terminal.font_family = family;
            assert_validation_error(config);
        }

        for font_size in [7.99, 40.01, f64::NAN, f64::INFINITY] {
            let mut config = AppConfig::default();
            config.terminal.font_size = font_size;
            assert_validation_error(config);
        }

        for line_height in [0.99, 2.01, f64::NEG_INFINITY] {
            let mut config = AppConfig::default();
            config.terminal.line_height = line_height;
            assert_validation_error(config);
        }

        for letter_spacing in [-2.01, 5.01, f64::NAN] {
            let mut config = AppConfig::default();
            config.terminal.letter_spacing = letter_spacing;
            assert_validation_error(config);
        }

        for font_weight in [99, 901] {
            let mut config = AppConfig::default();
            config.terminal.font_weight = font_weight;
            assert_validation_error(config);
        }
    }

    #[test]
    fn validation_requires_a_valid_coordinate_pair() {
        let cases = [
            (Some(0.0), None),
            (None, Some(0.0)),
            (Some(-90.01), Some(0.0)),
            (Some(90.01), Some(0.0)),
            (Some(0.0), Some(-180.01)),
            (Some(0.0), Some(180.01)),
            (Some(f64::NAN), Some(0.0)),
            (Some(0.0), Some(f64::INFINITY)),
        ];

        for (latitude, longitude) in cases {
            let mut config = AppConfig::default();
            config.appearance.latitude = latitude;
            config.appearance.longitude = longitude;
            assert_validation_error(config);
        }
    }

    #[test]
    fn missing_file_loads_defaults_without_creating_a_file() {
        let directory = TestDirectory::new();
        let path = directory.config_path();

        assert_eq!(load_from_path(&path).unwrap(), AppConfig::default());
        assert!(!path.exists());
    }

    #[test]
    fn malformed_or_invalid_files_return_sanitized_errors() {
        let directory = TestDirectory::new();
        let path = directory.config_path();
        fs::write(&path, b"{not json").unwrap();
        let error = load_from_path(&path).unwrap_err();
        assert_eq!(error.code, "config_parse_failed");
        assert!(!error.message.contains(path.to_string_lossy().as_ref()));

        fs::write(&path, br#"{"terminal":{"fontSize":500},"schemaVersion":1}"#).unwrap();
        assert_eq!(
            load_from_path(&path).unwrap_err().code,
            "config_validation_failed"
        );
    }

    #[test]
    fn save_and_load_round_trip_pretty_normalized_json() {
        let directory = TestDirectory::new();
        let path = directory.config_path();
        let mut config = AppConfig {
            vim_mode: true,
            animations: false,
            ..AppConfig::default()
        };
        config.appearance.mode = AppearanceMode::SunSchedule;
        config.appearance.latitude = Some(13.7563);
        config.appearance.longitude = Some(100.5018);
        config.terminal.font_family = "  Fira Code  ".to_owned();
        config.terminal.font_ligatures = false;

        let saved = save_to_path(&path, config).unwrap();
        assert_eq!(saved.terminal.font_family, "Fira Code");
        assert!(saved.vim_mode);
        assert_eq!(load_from_path(&path).unwrap(), saved);

        let source = fs::read_to_string(&path).unwrap();
        assert!(source.ends_with('\n'));
        assert!(source.contains("\n  \"schemaVersion\": 1,"));
        assert!(source.contains("\n    \"fontFamily\": \"Fira Code\","));
        assert!(source.contains("\n  \"vimMode\": true,"));
        assert_eq!(
            fs::read_dir(&directory.path).unwrap().count(),
            1,
            "temporary files must not remain after a successful save"
        );
    }

    #[test]
    fn invalid_save_preserves_existing_configuration() {
        let directory = TestDirectory::new();
        let path = directory.config_path();
        let original = AppConfig::default();
        save_to_path(&path, original.clone()).unwrap();
        let original_bytes = fs::read(&path).unwrap();

        let mut invalid = original;
        invalid.terminal.font_size = 100.0;
        assert_eq!(
            save_to_path(&path, invalid).unwrap_err().code,
            "config_validation_failed"
        );
        assert_eq!(fs::read(&path).unwrap(), original_bytes);
    }

    #[test]
    fn oversized_configuration_is_rejected() {
        let directory = TestDirectory::new();
        let path = directory.config_path();
        fs::write(&path, vec![b' '; MAX_CONFIG_BYTES as usize + 1]).unwrap();

        assert_eq!(load_from_path(&path).unwrap_err().code, "config_too_large");
    }

    fn assert_validation_error(config: AppConfig) {
        assert_eq!(
            config.normalize_and_validate().unwrap_err().code,
            "config_validation_failed"
        );
    }
}
