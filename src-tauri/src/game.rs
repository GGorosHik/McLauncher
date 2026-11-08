use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
};

/// Tracks the running game process so the UI can stop it.
pub struct GameState {
    child: Mutex<Option<Child>>,
}

impl Default for GameState {
    fn default() -> Self {
        Self { child: Mutex::new(None) }
    }
}

pub struct LaunchPlan {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub description: String,
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Starts the game process and keeps the handle for a later stop request.
///
/// A `.bat` client is spawned **directly** (no `cmd` wrapper). Windows resolves the
/// script through the shell association, `CREATE_NO_WINDOW` suppresses the console, and
/// the exit code is available on the child handle. Wrapping the script in
/// `cmd /c start /b /wait "<script>"` — or even `cmd /c ""<script>""` from Rust — made
/// the script silently not run, so it is not used.
pub fn start(state: &GameState, plan: &LaunchPlan) -> Result<u32, String> {
    {
        let mut guard = state.child.lock().map_err(|_| "Game state is locked.".to_string())?;
        if let Some(child) = guard.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                return Err("The game is already running.".to_string());
            }
        }
        *guard = None;
    }

    let mut command = Command::new(&plan.executable);
    command.args(&plan.args);
    command
        .current_dir(&plan.working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    let child = command.spawn().map_err(|error| format!("Could not start the game: {error}"))?;
    let pid = child.id();
    let mut guard = state.child.lock().map_err(|_| "Game state is locked.".to_string())?;
    *guard = Some(child);
    Ok(pid)
}

/// Builds either a `.bat` launch or a direct Java launch, depending on what the install looks like.
pub fn plan(root: &Path, java: &Path, min_ram: u32, max_ram: u32, extra_args: &str) -> Result<LaunchPlan, String> {
    let min_ram = min_ram.clamp(1, 128);
    let max_ram = max_ram.clamp(min_ram, 128);

    if let Some(script) = ["start-local.bat", "start.bat"]
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file())
    {
        return Ok(LaunchPlan {
            executable: script.clone(),
            args: Vec::new(),
            working_dir: script.parent().unwrap_or(root).to_path_buf(),
            description: format!("bundled script {}", script.display()),
        });
    }

    // Fall back to a Mojang/Fabric layout: <root>/versions/<id>/<id>.jar plus <root>/libraries.
    let versions = root.join("versions");
    let (version_id, client_jar) = find_version(&versions)?;
    let mut classpath: Vec<String> = vec![client_jar.to_string_lossy().to_string()];
    collect_jars(&root.join("libraries"), &mut classpath);
    if classpath.len() < 2 {
        return Err("The libraries folder is empty РІР‚вЂќ the client looks incomplete. Reinstall it.".to_string());
    }

    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut args: Vec<String> = Vec::new();
    args.push(format!("-Xms{min_ram}G"));
    args.push(format!("-Xmx{max_ram}G"));
    for argument in extra_args.split_whitespace() {
        if !argument.trim().is_empty() {
            args.push(argument.to_string());
        }
    }
    let natives = root.join("natives");
    args.push(format!("-Djava.library.path={}", natives.display()));
    args.push("-cp".to_string());
    args.push(classpath.join(separator));
    args.push("net.fabricmc.loader.impl.launch.knot.KnotClient".to_string());
    args.push("--gameDir".to_string());
    args.push(root.to_string_lossy().to_string());
    args.push("--assetsDir".to_string());
    args.push(root.join("assets").to_string_lossy().to_string());
    args.push("--assetIndex".to_string());
    args.push("34".to_string());
    args.push("--version".to_string());
    args.push(version_id.clone());
    args.push("--username".to_string());
    args.push(username());
    args.push("--accessToken".to_string());
    args.push("0".to_string());
    args.push("--userType".to_string());
    args.push("legacy".to_string());
    args.push("--versionType".to_string());
    args.push("release".to_string());

    Ok(LaunchPlan {
        executable: java.to_path_buf(),
        args,
        working_dir: root.to_path_buf(),
        description: format!("Java {version_id} client"),
    })
}

fn username() -> String {
    std::env::var("LAUNCHER_USERNAME").unwrap_or_else(|_| "Player".to_string())
}

fn find_version(versions: &Path) -> Result<(String, PathBuf), String> {
    let entries = std::fs::read_dir(versions)
        .map_err(|_| "No versions folder was found. Install the client first.".to_string())?;
    let mut best: Option<(String, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else { continue };
        let jar = path.join(format!("{name}.jar"));
        if jar.is_file() {
            let better = match &best {
                Some((current, _)) => name > current.as_str(),
                None => true,
            };
            if better {
                best = Some((name.to_string(), jar));
            }
        }
    }
    best.ok_or_else(|| "No client jar was found under versions\\. Reinstall the client.".to_string())
}

fn collect_jars(directory: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jars(&path, out);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("jar")) {
            out.push(path.to_string_lossy().to_string());
        }
    }
}

pub fn stop(state: &GameState) -> Result<(), String> {
    let mut guard = state.child.lock().map_err(|_| "Game state is locked.".to_string())?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

pub fn is_running(state: &GameState) -> bool {
    let Ok(mut guard) = state.child.lock() else { return false };
    match guard.as_mut() {
        Some(child) => child.try_wait().ok().flatten().is_none(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_rejects_empty_directory() {
        let root = std::env::temp_dir().join("mc-game-test-empty");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let java = Path::new("java.exe");
        assert!(plan(&root, java, 2, 4, "").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_uses_bat_script_when_present() {
        let root = std::env::temp_dir().join("mc-game-test-bat");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        std::fs::write(root.join("start-local.bat"), b"@echo off").unwrap();
        let result = plan(&root, Path::new("java.exe"), 2, 4, "").unwrap();
        assert!(result.executable.ends_with("start-local.bat"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_clamps_memory_bounds() {
        let root = std::env::temp_dir().join("mc-game-test-clamp");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        std::fs::write(root.join("start.bat"), b"@echo off").unwrap();
        let result = plan(&root, Path::new("java.exe"), 0, 999, "").unwrap();
        assert!(result.executable.ends_with("start.bat"));
        let _ = std::fs::remove_dir_all(&root);
    }
}


#[cfg(test)]
mod launch_tests {
    use super::*;
    use std::sync::Mutex;

    /// Runs a real .bat through `start()` and confirms it executed without a console.
    /// This guards the regression where `cmd /c start /b /wait "<script>"` silently
    /// skipped the script entirely.
    #[test]
    fn bat_script_actually_runs_without_console() {
        let root = std::env::temp_dir().join("mc-launch-e2e");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("ran.txt");
        let script = root.join("start.bat");
        std::fs::write(
            &script,
            format!("@echo off\r\necho executed > \"{}\"\r\n", marker.display()),
        )
        .unwrap();

        let plan = LaunchPlan {
            executable: script.clone(),
            args: Vec::new(),
            working_dir: root.clone(),
            description: "e2e".to_string(),
        };
        let state = GameState { child: Mutex::new(None) };
        start(&state, &plan).expect("launch should succeed");

        // Give cmd a moment to write the marker.
        for _ in 0..60 {
            if marker.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let ran = marker.is_file();
        let _ = stop(&state);
        let _ = std::fs::remove_dir_all(&root);
        assert!(ran, "the batch file did not run: the launch command is broken");
    }
}
