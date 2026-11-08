use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

/// A file the player dropped into a mods folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMod {
    pub file_name: String,
    pub size: u64,
    pub enabled: bool,
}

/// A Modrinth search hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModHit {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    pub icon_url: String,
    pub author: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Option<Vec<SearchHit>>,
}

#[derive(Deserialize)]
struct SearchHit {
    project_id: Option<String>,
    slug: Option<String>,
    title: Option<String>,
    description: Option<String>,
    downloads: Option<u64>,
    icon_url: Option<String>,
    author: Option<String>,
}

#[derive(Deserialize)]
struct VersionListEntry {
    files: Option<Vec<VersionFile>>,
}

#[derive(Deserialize)]
struct VersionFile {
    url: Option<String>,
    filename: Option<String>,
}

const USER_AGENT: &str = "McLauncher/0.1.0";
const MODRINTH_API: &str = "https://api.modrinth.com/v2";

/// The mods folder for a client root. Creates it when missing.
pub fn mods_dir(root: &Path) -> Result<PathBuf, String> {
    let candidates = [root.join("game").join("mods"), root.join("mods")];
    for candidate in candidates.iter() {
        if candidate.is_dir() {
            return Ok(candidate.clone());
        }
    }
    // Prefer <root>/game/mods for a bundle layout, <root>/mods otherwise.
    let target = if root.join("game").is_dir() {
        root.join("game").join("mods")
    } else {
        root.join("mods")
    };
    fs::create_dir_all(&target).map_err(|error| format!("Could not create {}: {error}", target.display()))?;
    Ok(target)
}

/// Lists `.jar` files, treating a `.jar.disabled` suffix as a disabled mod.
pub fn list(root: &Path) -> Result<Vec<LocalMod>, String> {
    let dir = mods_dir(root)?;
    let entries = fs::read_dir(&dir).map_err(|error| format!("Could not read {}: {error}", dir.display()))?;
    let mut mods = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else { continue };
        let lower = name.to_ascii_lowercase();
        let (base, enabled) = if lower.ends_with(".jar.disabled") {
            (name.trim_end_matches(".disabled").to_string(), false)
        } else if lower.ends_with(".jar") {
            (name.to_string(), true)
        } else {
            continue;
        };
        let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        mods.push(LocalMod { file_name: base, size, enabled });
    }
    mods.sort_by(|left, right| left.file_name.to_lowercase().cmp(&right.file_name.to_lowercase()));
    Ok(mods)
}

/// Renames a mod between `<name>.jar` and `<name>.jar.disabled`.
pub fn set_enabled(root: &Path, file_name: &str, enabled: bool) -> Result<(), String> {
    let dir = mods_dir(root)?;
    let safe = sanitize_file_name(file_name)?;
    let active = dir.join(&safe);
    let disabled = dir.join(format!("{safe}.disabled"));
    if enabled {
        if disabled.is_file() {
            fs::rename(&disabled, &active).map_err(|error| format!("Could not enable {safe}: {error}"))?;
        } else if !active.is_file() {
            return Err(format!("{safe} was not found in the mods folder."));
        }
    } else if active.is_file() {
        fs::rename(&active, &disabled).map_err(|error| format!("Could not disable {safe}: {error}"))?;
    }
    Ok(())
}

/// Deletes a mod file (enabled or disabled).
pub fn remove(root: &Path, file_name: &str) -> Result<(), String> {
    let dir = mods_dir(root)?;
    let safe = sanitize_file_name(file_name)?;
    let active = dir.join(&safe);
    let disabled = dir.join(format!("{safe}.disabled"));
    let mut removed = false;
    for candidate in [active, disabled] {
        if candidate.is_file() {
            fs::remove_file(&candidate).map_err(|error| format!("Could not delete {}: {error}", candidate.display()))?;
            removed = true;
        }
    }
    if !removed {
        return Err(format!("{safe} was not found."));
    }
    Ok(())
}

fn sanitize_file_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err("Mod file name is invalid.".to_string());
    }
    if trimmed.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || trimmed.contains("..") {
        return Err("Mod file name contains illegal characters.".to_string());
    }
    Ok(trimmed.to_string())
}

/// Copies a local `.jar` picked by the user into the mods folder.
pub fn add_local(root: &Path, source: &str) -> Result<String, String> {
    let source_path = PathBuf::from(source.trim());
    if !source_path.is_file() {
        return Err(format!("File not found: {}", source_path.display()));
    }
    let Some(name) = source_path.file_name().and_then(|value| value.to_str()) else {
        return Err("That file has no usable name.".to_string());
    };
    if !name.to_ascii_lowercase().ends_with(".jar") {
        return Err("Only .jar mod files can be added.".to_string());
    }
    let safe = sanitize_file_name(name)?;
    let dir = mods_dir(root)?;
    let destination = dir.join(&safe);
    if destination.exists() {
        return Err(format!("{safe} is already installed."));
    }
    fs::copy(&source_path, &destination)
        .map_err(|error| format!("Could not copy {safe}: {error}"))?;
    Ok(safe)
}

/// Searches Modrinth for Fabric-compatible mods.
pub fn search(query: &str, game_version: &str) -> Result<Vec<ModHit>, String> {
    let text = query.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let facets = if game_version.trim().is_empty() {
        "[[\"project_type:mod\"],[\"categories:fabric\"]]".to_string()
    } else {
        format!(
            "[[\"project_type:mod\"],[\"categories:fabric\"],[\"versions:{}\"]]",
            game_version.trim()
        )
    };
    let url = format!(
        "{MODRINTH_API}/search?query={}&limit=20&facets={}&index=relevance",
        urlencoding(text),
        urlencoding(&facets)
    );
    let body = http_get(&url)?;
    let parsed: SearchResponse = serde_json::from_str(&body)
        .map_err(|error| format!("Unexpected Modrinth response: {error}"))?;
    Ok(parsed
        .hits
        .unwrap_or_default()
        .into_iter()
        .filter_map(|hit| {
            Some(ModHit {
                id: hit.project_id?,
                slug: hit.slug.unwrap_or_default(),
                title: hit.title.unwrap_or_default(),
                description: hit.description.unwrap_or_default(),
                downloads: hit.downloads.unwrap_or(0),
                icon_url: hit.icon_url.unwrap_or_default(),
                author: hit.author.unwrap_or_default(),
            })
        })
        .collect())
}

/// Downloads the newest Fabric file for a project into the mods folder.
pub fn install_from_modrinth(root: &Path, project_id: &str, game_version: &str) -> Result<String, String> {
    let version_filter = if game_version.trim().is_empty() {
        String::new()
    } else {
        format!(
            "&loaders={}&game_versions={}",
            urlencoding("[\"fabric\"]"),
            urlencoding(&format!("[\"{}\"]", game_version.trim()))
        )
    };
    let list_url = format!("{MODRINTH_API}/project/{project_id}/version{version_filter}");
    let body = http_get(&list_url)?;
    let versions: Vec<VersionListEntry> =
        serde_json::from_str(&body).map_err(|error| format!("Unexpected Modrinth response: {error}"))?;
    let file = versions
        .iter()
        .find_map(|version| version.files.as_ref().and_then(|files| files.first()))
        .ok_or_else(|| "No downloadable file was found for this mod.".to_string())?;
    let url = file.url.clone().ok_or_else(|| "The mod file has no download URL.".to_string())?;
    let file_name = file
        .filename
        .clone()
        .ok_or_else(|| "The mod file has no name.".to_string())?;
    let safe = sanitize_file_name(&file_name)?;

    let dir = mods_dir(root)?;
    let destination = dir.join(&safe);
    let temp = dir.join(format!("{safe}.part"));
    download_to(&url, &temp)?;
    if destination.exists() {
        let _ = fs::remove_file(&destination);
    }
    fs::rename(&temp, &destination).map_err(|error| format!("Could not save {safe}: {error}"))?;
    Ok(safe)
}

fn download_to(url: &str, destination: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("Download failed: {error}"))?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(destination)
        .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
    io::copy(&mut reader, &mut file).map_err(|error| format!("Could not write {destination:?}: {error}"))?;
    Ok(())
}

fn http_get(url: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("Modrinth request failed: {error}"))?;
    let mut text = String::new();
    response
        .into_reader()
        .read_to_string(&mut text)
        .map_err(|error| format!("Could not read the response: {error}"))?;
    Ok(text)
}

/// Percent-encodes a query-string value.
fn urlencoding(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => encoded.push(byte as char),
            b' ' => encoded.push_str("%20"),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_names() {
        assert!(sanitize_file_name("../evil.jar").is_err());
        assert!(sanitize_file_name("a/b.jar").is_err());
        assert!(sanitize_file_name("ok-mod.jar").is_ok());
    }

    #[test]
    fn percent_encodes_reserved_characters() {
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("[\"fabric\"]"), "%5B%22fabric%22%5D");
    }

    #[test]
    fn mods_dir_prefers_game_folder_when_bundle_layout() {
        let root = std::env::temp_dir().join("mc-mods-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("game")).unwrap();
        let dir = mods_dir(&root).unwrap();
        assert!(dir.ends_with("mods"));
        assert!(dir.to_string_lossy().contains("game"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enable_disable_round_trip() {
        let root = std::env::temp_dir().join("mc-mods-toggle");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dir = mods_dir(&root).unwrap();
        fs::write(dir.join("test.jar"), b"stub").unwrap();
        set_enabled(&root, "test.jar", false).unwrap();
        assert!(dir.join("test.jar.disabled").is_file());
        let listed = list(&root).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].enabled);
        set_enabled(&root, "test.jar", true).unwrap();
        assert!(dir.join("test.jar").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}

