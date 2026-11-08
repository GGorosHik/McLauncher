#![cfg_attr(windows, windows_subsystem = "windows")]

mod config;
mod game;
mod java;
mod mods;
mod update;

use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
};
use tauri::{AppHandle, Emitter, Manager, State};

use config::Settings;
use game::GameState;

const PROGRESS_EVENT: &str = "install://progress";

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    stage: String,
    message: String,
    percent: u8,
}

fn emit(app: &AppHandle, stage: &str, message: impl Into<String>, percent: u8) {
    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressEvent { stage: stage.to_string(), message: message.into(), percent: percent.min(100) },
    );
}

#[derive(Clone, Default)]
struct BusyFlag(Arc<AtomicBool>);

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    settings: Settings,
    layout: String,
    client_present: bool,
    launcher_script: Option<String>,
    system_ram_gb: u32,
    java: Vec<java::JavaRuntime>,
    running: bool,
}

fn layout_label(root: &PathBuf) -> String {
    let layout = config::detect_layout(root);
    if layout.launcher_bat.is_some() {
        "bundle".to_string()
    } else if root.join("libraries").is_dir() {
        "classpath".to_string()
    } else {
        "empty".to_string()
    }
}

#[tauri::command]
fn get_app_info(app: AppHandle, state: State<'_, GameState>) -> Result<AppInfo, String> {
    let settings = config::load_settings(&app)?;
    let root = PathBuf::from(&settings.install_dir);
    let layout = config::detect_layout(&root);
    let client_present = layout.launcher_bat.is_some() || root.join("libraries").is_dir();
    Ok(AppInfo {
        launcher_script: layout.launcher_bat.as_ref().map(|path| path.to_string_lossy().to_string()),
        layout: layout_label(&root),
        client_present,
        system_ram_gb: config::system_memory_gb(),
        java: java::detect(),
        running: game::is_running(&state),
        settings,
    })
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let mut next = settings;
    if next.max_ram_gb > next.min_ram_gb {
        std::mem::swap(&mut next.min_ram_gb, &mut next.max_ram_gb);
    }
    next.min_ram_gb = next.min_ram_gb.clamp(1, 64);
    next.max_ram_gb = next.max_ram_gb.clamp(next.min_ram_gb, 64);
    config::save_settings(&app, &next)?;
    Ok(next)
}

#[tauri::command]
async fn pick_directory(app: AppHandle, start: String) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new().set_title("Choose a folder for the Minecraft client");
    let start_path = PathBuf::from(start.trim());
    if start_path.is_dir() {
        dialog = dialog.set_directory(&start_path);
    } else if let Some(parent) = start_path.parent().filter(|path| path.is_dir()) {
        dialog = dialog.set_directory(parent);
    }
    // Report the pick back to the window that asked for it. The dialog is spawned on a
    // blocking pool thread so the webview stays responsive while it is open.
    let window = app.get_webview_window("main").ok_or_else(|| "The main window is gone.".to_string())?;
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.pick_folder())
        .await
        .map_err(|error| format!("Folder dialog failed: {error}"))?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let value = path.to_string_lossy().to_string();
    window
        .emit("dialog://folder", value.clone())
        .map_err(|error| format!("Could not deliver the folder: {error}"))?;
    Ok(Some(value))
}

#[tauri::command]
async fn pick_java(app: AppHandle) -> Result<Option<String>, String> {
    let window = app.get_webview_window("main").ok_or_else(|| "The main window is gone.".to_string())?;
    let picked = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Choose java.exe")
            .add_filter("Java executable", &["exe"])
            .pick_file()
    })
    .await
    .map_err(|error| format!("File dialog failed: {error}"))?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let value = path.to_string_lossy().to_string();
    window
        .emit("dialog://java", value.clone())
        .map_err(|error| format!("Could not deliver the path: {error}"))?;
    Ok(Some(value))
}

#[tauri::command]
fn check_update(app: AppHandle, repo: String, tag: String) -> Result<update::UpdateInfo, String> {
    let settings = config::load_settings(&app)?;
    let repo = if repo.trim().is_empty() { settings.repo } else { repo };
    if repo.trim().is_empty() {
        return Err("Set the GitHub repository (owner/repo) before checking for updates.".to_string());
    }
    update::check(&repo, &tag, &settings.installed_tag)
}

#[tauri::command]
async fn download_and_install(
    app: AppHandle,
    busy: State<'_, BusyFlag>,
    repo: String,
    tag: String,
    target_dir: String,
) -> Result<Settings, String> {
    if busy.0.swap(true, Ordering::SeqCst) {
        return Err("An install is already running.".to_string());
    }
    let result = (|| -> Result<Settings, String> {
        let mut settings = config::load_settings(&app)?;
        let info = check_update(app.clone(), repo.clone(), tag.clone())?;
        if target_dir.trim().is_empty() {
            return Err("Choose an install folder first.".to_string());
        }
        let target = PathBuf::from(target_dir.trim());
        std::fs::create_dir_all(&target)
            .map_err(|error| format!("Could not create {}: {error}", target.display()))?;

        let staging = std::env::temp_dir().join(format!(
            "mclauncher-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&staging).map_err(|error| format!("Could not create staging: {error}"))?;
        let archive = staging.join(&info.asset.name);

        emit(&app, "download", format!("Downloading {} ({})", info.asset.name, human_size(info.asset.size)), 5);
        let app_for_progress = app.clone();
        update::download(&info.asset.url, &archive, info.asset.size, move |written, total| {
            let percent = 5 + ((written as f64 / total.max(1) as f64) * 80.0) as u8;
            emit(&app_for_progress, "download", format!("{}/{}", human_size(written), human_size(total)), percent.min(85));
        })?;

        emit(&app, "extract", "Extracting the client…", 88);
        update::install_from_archive(&archive, &staging, &target)?;
        let _ = std::fs::remove_dir_all(&staging);

        emit(&app, "done", format!("Installed {}", info.tag), 100);
        settings.install_dir = target.to_string_lossy().to_string();
        settings.installed_tag = info.tag.clone();
        if !info.repo.trim().is_empty() {
            settings.repo = info.repo.clone();
        }
        config::save_settings(&app, &settings)?;
        Ok(settings)
    })();
    busy.0.store(false, Ordering::SeqCst);
    result
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[tauri::command]
fn launch_game(
    app: AppHandle,
    state: State<'_, GameState>,
    settings: Settings,
) -> Result<String, String> {
    let mut settings = settings;
    settings.min_ram_gb = settings.min_ram_gb.clamp(1, 128);
    settings.max_ram_gb = settings.max_ram_gb.clamp(settings.min_ram_gb, 128);
    config::save_settings(&app, &settings)?;

    let root = PathBuf::from(settings.install_dir.trim());
    if !root.is_dir() {
        return Err(format!("The install folder does not exist: {}", root.display()));
    }
    let java = java::resolve(&settings.java_path, &root)?;
    let plan = game::plan(&root, &java, settings.min_ram_gb, settings.max_ram_gb, &settings.java_args)?;
    let pid = game::start(&state, &plan)?;
    Ok(format!("Started {} (pid {pid})", plan.description))
}

#[tauri::command]
fn stop_game(state: State<'_, GameState>) -> Result<(), String> {
    game::stop(&state)
}

#[tauri::command]
fn is_game_running(state: State<'_, GameState>) -> bool {
    game::is_running(&state)
}

#[tauri::command]
fn mods_dir(app: AppHandle) -> Result<String, String> {
    let settings = config::load_settings(&app)?;
    let dir = mods::mods_dir(PathBuf::from(settings.install_dir.trim()).as_path())?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
fn list_mods(app: AppHandle) -> Result<Vec<mods::LocalMod>, String> {
    let settings = config::load_settings(&app)?;
    mods::list(PathBuf::from(settings.install_dir.trim()).as_path())
}

#[tauri::command]
fn set_mod_enabled(app: AppHandle, file_name: String, enabled: bool) -> Result<(), String> {
    let settings = config::load_settings(&app)?;
    mods::set_enabled(PathBuf::from(settings.install_dir.trim()).as_path(), &file_name, enabled)
}

#[tauri::command]
fn remove_mod(app: AppHandle, file_name: String) -> Result<(), String> {
    let settings = config::load_settings(&app)?;
    mods::remove(PathBuf::from(settings.install_dir.trim()).as_path(), &file_name)
}

#[tauri::command]
async fn pick_mod_file(app: AppHandle) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Choose a mod (.jar)")
            .add_filter("Minecraft mod", &["jar"])
            .pick_files()
    })
    .await
    .map_err(|error| format!("File dialog failed: {error}"))?;
    let Some(paths) = picked else { return Ok(None) };
    let mut added = Vec::new();
    let settings = config::load_settings(&app)?;
    let root = PathBuf::from(settings.install_dir.trim());
    for path in paths {
        let source = path.to_string_lossy().to_string();
        if let Ok(name) = mods::add_local(&root, &source) {
            added.push(name);
        }
    }
    Ok(Some(added.join("\n")))
}

#[tauri::command]
fn search_mods(query: String, game_version: String) -> Result<Vec<mods::ModHit>, String> {
    mods::search(&query, &game_version)
}

#[tauri::command]
fn install_mod_from_modrinth(app: AppHandle, project_id: String, game_version: String) -> Result<String, String> {
    let settings = config::load_settings(&app)?;
    mods::install_from_modrinth(PathBuf::from(settings.install_dir.trim()).as_path(), &project_id, &game_version)
}

#[tauri::command]
fn open_mods_folder(app: AppHandle) -> Result<String, String> {
    let settings = config::load_settings(&app)?;
    let dir = mods::mods_dir(PathBuf::from(settings.install_dir.trim()).as_path())?;
    let path = dir.to_string_lossy().to_string();
    let _ = tauri_plugin_opener::open_path(&path, None::<&str>);
    Ok(path)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(GameState::default())
        .manage(BusyFlag::default())
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            save_settings,
            pick_directory,
            pick_java,
            check_update,
            download_and_install,
            launch_game,
            stop_game,
            is_game_running,
            mods_dir,
            list_mods,
            set_mod_enabled,
            remove_mod,
            pick_mod_file,
            search_mods,
            install_mod_from_modrinth,
            open_mods_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the launcher");
}
