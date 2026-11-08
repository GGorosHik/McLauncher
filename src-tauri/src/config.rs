use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{path::BaseDirectory, AppHandle, Manager};

/// Settings the user can change from the UI. Persisted as JSON next to the app data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Folder the client is installed into and launched from.
    pub install_dir: String,
    /// Minimum heap, in gigabytes.
    pub min_ram_gb: u32,
    /// Maximum heap, in gigabytes.
    pub max_ram_gb: u32,
    /// Explicit java.exe path; empty means auto-detect.
    pub java_path: String,
    /// JVM flags appended before the classpath.
    pub java_args: String,
    /// "owner/repo" used for update checks.
    pub repo: String,
    /// Release tag currently installed, empty when unknown.
    pub installed_tag: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            install_dir: String::new(),
            min_ram_gb: 2,
            max_ram_gb: 4,
            java_path: String::new(),
            java_args: String::new(),
            repo: String::new(),
            installed_tag: String::new(),
        }
    }
}

/// Where the launcher may find a playable client.
#[derive(Debug, Clone)]
pub struct ClientLayout {
    pub launcher_bat: Option<PathBuf>,
}

const SETTINGS_FILE: &str = "settings.json";
const DEFAULT_DIR_NAME: &str = "Minecraft";

/// Where client updates come from. Fixed at build time and never shown in the UI.
pub const DEFAULT_REPO: &str = "GGorosHik/McLauncher";

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .resolve("", BaseDirectory::AppConfig)
        .map_err(|error| format!("Could not resolve the launcher config directory: {error}"))?;
    fs::create_dir_all(&dir).map_err(|error| format!("Could not create {}: {error}", dir.display()))?;
    Ok(dir.join(SETTINGS_FILE))
}

pub fn load_settings(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    let mut settings = match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str::<Settings>(&raw).unwrap_or_default(),
        Err(_) => Settings::default(),
    };
    if settings.install_dir.trim().is_empty() {
        settings.install_dir = default_install_dir(app)?.to_string_lossy().to_string();
    }
    // The download source is baked into the build; it is never shown or edited in the UI.
    settings.repo = DEFAULT_REPO.to_string();
    Ok(settings)
}

pub fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(settings).map_err(|error| format!("Could not encode settings: {error}"))?;
    fs::write(&path, raw).map_err(|error| format!("Could not write {}: {error}", path.display()))
}

fn default_install_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|error| format!("Could not resolve a documents directory: {error}"))?;
    Ok(base.join(DEFAULT_DIR_NAME))
}

/// Detects how a directory can be launched: a bundled script first, then a version jar layout.
pub fn detect_layout(dir: &Path) -> ClientLayout {
    let launcher_bat = ["start-local.bat", "start.bat"]
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
        .or_else(|| find_bat_recursive(dir, 0));
    ClientLayout { launcher_bat }
}

fn find_bat_recursive(dir: &Path, depth: usize) -> Option<PathBuf> {
    if depth > 3 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    let mut directories = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            directories.push(path);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("bat")) {
            return Some(path);
        }
    }
    directories.into_iter().find_map(|child| find_bat_recursive(&child, depth + 1))
}

/// Detects a machine-appropriate memory ceiling, rounded down to whole gigabytes.
pub fn system_memory_gb() -> u32 {
    #[cfg(target_os = "windows")]
    {
        use std::mem::MaybeUninit;
        #[repr(C)]
        struct MemoryStatusEx {
            length: u32,
            memory_load: u32,
            total_phys: u64,
            avail_phys: u64,
            total_page_file: u64,
            avail_page_file: u64,
            total_virtual: u64,
            avail_virtual: u64,
            avail_extended_virtual: u64,
        }
        extern "system" {
            fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
        }
        let mut status = MaybeUninit::<MemoryStatusEx>::zeroed();
        unsafe {
            let pointer = status.as_mut_ptr();
            (*pointer).length = std::mem::size_of::<MemoryStatusEx>() as u32;
            if GlobalMemoryStatusEx(pointer) != 0 {
                let total = (*pointer).total_phys;
                return (total / (1024 * 1024 * 1024)) as u32;
            }
        }
    }
    8
}
