use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub theme: String,
    pub font_family: String,
    pub font_size: f64,
    pub ligatures: bool,
    pub shell: Option<String>,
    pub vim_mode: bool,
    pub reduce_motion: bool,
    pub raw_pty: bool,
    pub abbreviations: HashMap<String, String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut abbreviations = HashMap::new();
        abbreviations.insert("ll".into(), "ls -la".into());
        abbreviations.insert("gco".into(), "git checkout".into());
        abbreviations.insert("gs".into(), "git status".into());
        Self {
            theme: "system".into(),
            font_family: "JetBrains Mono".into(),
            font_size: 13.5,
            ligatures: true,
            shell: None,
            vim_mode: true,
            reduce_motion: false,
            raw_pty: false,
            abbreviations,
        }
    }
}

pub fn app_dir() -> Result<PathBuf, String> {
    let dir = dirs::config_dir()
        .ok_or_else(|| "could not resolve config dir".to_string())?
        .join("twominal");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn data_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .ok_or_else(|| "could not resolve data dir".to_string())?
        .join("twominal");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn config_path() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("config.json"))
}

#[tauri::command]
pub fn config_get() -> Result<AppConfig, String> {
    let path = config_path()?;
    if !path.exists() {
        let cfg = AppConfig::default();
        let _ = config_set(cfg.clone());
        return Ok(cfg);
    }
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut cfg: AppConfig = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if cfg.theme == "sunset-sunrise" {
        cfg.theme = "system".into();
    }
    Ok(cfg)
}

#[tauri::command]
pub fn config_set(cfg: AppConfig) -> Result<(), String> {
    let path = config_path()?;
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}
