use crate::fish::{CommandCheckResult, CompletionItem, FishEngine, ShellExecResult};
use crate::pty::PtyManager;
use crate::theme::{SolarManager, SolarThemeInfo};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemInfo {
    pub os: String,
    pub host: String,
    pub user: String,
    pub shell: String,
    pub home_dir: String,
    pub cwd: String,
    pub display_cwd: String,
    pub kernel: String,
    pub arch: String,
}

#[tauri::command]
pub fn shell_exec(app: AppHandle, tab_id: Option<String>, cwd: String, command: String) -> Result<ShellExecResult, String> {
    FishEngine::exec_with_app(Some(&app), tab_id.as_deref(), &cwd, &command)
}

#[tauri::command]
pub fn shell_write(tab_id: String, data: String) -> Result<(), String> {
    FishEngine::write_stdin(&tab_id, &data)
}

#[tauri::command]
pub fn shell_cancel(tab_id: String) -> Result<(), String> {
    FishEngine::cancel(&tab_id);
    Ok(())
}


#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    state: State<'_, Arc<PtyManager>>,
    id: String,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    shell: Option<String>,
    command: Option<String>,
) -> Result<(), String> {
    state.spawn(app, id, cols, rows, cwd, shell, command)
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, Arc<PtyManager>>,
    id: String,
    data: String,
) -> Result<(), String> {
    state.write(&id, &data)
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, Arc<PtyManager>>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.resize(&id, cols, rows)
}

#[tauri::command]
pub fn pty_kill(
    state: State<'_, Arc<PtyManager>>,
    id: String,
) -> Result<(), String> {
    state.kill(&id)
}

#[tauri::command]
pub fn fish_check_command(command: String) -> CommandCheckResult {
    FishEngine::check_command(&command)
}

#[tauri::command]
pub fn fish_get_completions(cwd: String, prefix: String) -> Vec<CompletionItem> {
    FishEngine::get_completions(&cwd, &prefix)
}

#[tauri::command]
pub fn fish_get_history(limit: Option<usize>) -> Vec<String> {
    FishEngine::get_history(limit.unwrap_or(200))
}

#[tauri::command]
pub fn fish_add_history(command: String) -> Result<(), String> {
    FishEngine::add_history(&command)
}

#[tauri::command]
pub fn get_solar_theme_info() -> SolarThemeInfo {
    SolarManager::get_solar_info()
}

#[tauri::command]
pub fn get_git_branch(cwd: String) -> Option<String> {
    FishEngine::get_git_branch(&cwd)
}

#[tauri::command]
pub fn get_system_info(cwd: Option<String>) -> SystemInfo {
    let os_raw = if cfg!(target_os = "macos") {
        "macOS".to_string()
    } else if cfg!(target_os = "windows") {
        "Windows".to_string()
    } else if cfg!(target_os = "linux") {
        "Linux".to_string()
    } else {
        std::env::consts::OS.to_string()
    };

    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "twominal-host".to_string());

    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());

    let shell = if cfg!(target_os = "windows") {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    };

    let home_dir_buf = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let home_dir = home_dir_buf.to_string_lossy().to_string();

    let target_cwd_buf = match cwd {
        Some(ref dir) if !dir.trim().is_empty() => FishEngine::expand_path(dir),
        _ => home_dir_buf.clone(),
    };

    let cwd = target_cwd_buf.to_string_lossy().to_string();
    let display_cwd = FishEngine::display_path(&target_cwd_buf);
    let arch = std::env::consts::ARCH.to_string();
    let kernel = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);

    SystemInfo {
        os: os_raw,
        host,
        user,
        shell,
        home_dir,
        cwd,
        display_cwd,
        kernel,
        arch,
    }
}

#[tauri::command]
pub fn window_minimize(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn window_toggle_maximize(window: tauri::Window) -> Result<(), String> {
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())?;
    } else {
        window.maximize().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn window_close(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn window_create_new(
    app: AppHandle,
    label: String,
    title: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), String> {
    let title_str = title.unwrap_or_else(|| "Twominal".to_string());
    let mut builder = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::default())
        .title(&title_str)
        .inner_size(width.unwrap_or(1000.0), height.unwrap_or(650.0))
        .min_inner_size(640.0, 420.0)
        .focused(true);

    if let (Some(px), Some(py)) = (x, y) {
        builder = builder.position(px, py);
    }

    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_system_info_default_cwd() {
        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let home_str = home.to_string_lossy().to_string();

        let info_none = get_system_info(None);
        assert_eq!(info_none.home_dir, home_str);
        assert_eq!(info_none.cwd, home_str);
        assert_eq!(info_none.display_cwd, "~");

        let info_empty = get_system_info(Some("".to_string()));
        assert_eq!(info_empty.cwd, home_str);
        assert_eq!(info_empty.display_cwd, "~");

        let info_tilde = get_system_info(Some("~".to_string()));
        assert_eq!(info_tilde.cwd, home_str);
        assert_eq!(info_tilde.display_cwd, "~");
    }
}
