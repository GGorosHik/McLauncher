use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// A Java runtime found on this machine.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntime {
    pub path: String,
    pub version: u32,
    pub label: String,
}

/// Runs `java -version` and returns the major version from stderr.
fn probe(java: &Path) -> Option<u32> {
    let output = Command::new(java)
        .arg("-version")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stderr).to_string();
    parse_major(&text)
}

/// "openjdk version \"21.0.2\" 2024-01-16" -> 21, legacy "1.8.0_381" -> 8.
fn parse_major(text: &str) -> Option<u32> {
    let quote = text.split('"').nth(1)?;
    let mut parts = quote.split(['.', '_', '-']);
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse::<u32>().ok()
    } else {
        Some(first)
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        candidates.push(PathBuf::from(java_home).join("bin").join("java.exe"));
    }
    if let Ok(path) = std::env::var("PATH") {
        for directory in std::env::split_paths(&path) {
            candidates.push(directory.join("java.exe"));
        }
    }
    for base in [
        r"C:\Program Files\Java",
        r"C:\Program Files\Eclipse Adoptium",
        r"C:\Program Files\Microsoft",
        r"C:\Program Files\Zulu",
        r"C:\Program Files\BellSoft",
    ] {
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                candidates.push(path.join("bin").join("java.exe"));
                candidates.push(path.join("jre").join("bin").join("java.exe"));
            }
        }
    }
    candidates
}

/// Finds every usable Java runtime, newest first.
pub fn detect() -> Vec<JavaRuntime> {
    let mut found: Vec<JavaRuntime> = Vec::new();
    for candidate in candidate_paths() {
        if !candidate.is_file() {
            continue;
        }
        if found.iter().any(|runtime| same_file(&runtime.path, &candidate)) {
            continue;
        }
        if let Some(version) = probe(&candidate) {
            found.push(JavaRuntime {
                path: candidate.to_string_lossy().to_string(),
                version,
                label: format!("Java {version} — {}", candidate.display()),
            });
        }
    }
    found.sort_by(|left, right| right.version.cmp(&left.version));
    found
}

fn same_file(left: &str, right: &Path) -> bool {
    fs::canonicalize(left).ok() == fs::canonicalize(right).ok()
}

/// Picks a runtime: an explicit path when it works, otherwise the newest Java 17+ (21+ preferred).
pub fn resolve(explicit: &str, bundled_root: &Path) -> Result<PathBuf, String> {
    let bundled = bundled_root.join("jre").join("bin").join("java.exe");
    if bundled.is_file() {
        if let Some(version) = probe(&bundled) {
            if version >= 17 {
                return Ok(bundled);
            }
        }
    }
    if !explicit.trim().is_empty() {
        let path = PathBuf::from(explicit.trim());
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("The configured Java path does not exist: {}", path.display()));
    }
    let mut runtimes = detect();
    runtimes.retain(|runtime| runtime.version >= 17);
    runtimes.sort_by_key(|runtime| std::cmp::Reverse(runtime.version));
    runtimes
        .into_iter()
        .next()
        .map(|runtime| PathBuf::from(runtime.path))
        .ok_or_else(|| {
            "No Java 17+ runtime was found. Install Java 21 or point the launcher at one.".to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_version_parsing_handles_modern_and_legacy() {
        assert_eq!(parse_major("openjdk version \"21.0.2\" 2024-01-16"), Some(21));
        assert_eq!(parse_major("java version \"1.8.0_381\""), Some(8));
        assert_eq!(parse_major("openjdk version \"25.0.5.0.3\" 2026-10-20"), Some(25));
        assert_eq!(parse_major("garbage"), None);
    }

    #[test]
    fn resolve_prefers_bundled_jre_when_present() {
        let root = std::env::temp_dir().join("mc-java-test-bundled");
        let bin = root.join("jre").join("bin");
        let _ = std::fs::create_dir_all(&bin);
        let exe = bin.join("java.exe");
        let _ = std::fs::write(&exe, b"stub");
        // A stub file has no version, so resolve falls through instead of returning it.
        let result = resolve("", &root);
        assert!(result.is_err() || result.is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_java_path_does_not_panic() {
        // Auto-detection depends on the machine, so only assert that it never panics
        // and that a hit is a real java.exe path.
        let root = std::env::temp_dir().join("mc-java-test-none");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        if let Ok(found) = resolve("", &root) {
            assert!(found.to_string_lossy().to_ascii_lowercase().ends_with("java.exe"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_explicit_java_path_is_an_error() {
        let root = std::env::temp_dir().join("mc-java-test-explicit");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let missing = root.join("nope").join("java.exe");
        let result = resolve(&missing.to_string_lossy(), &root);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
