use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// A release asset the launcher can install.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleAsset {
    pub name: String,
    pub size: u64,
    pub url: String,
}

/// Result of an update check against the GitHub Releases API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub repo: String,
    pub tag: String,
    pub name: String,
    pub notes: String,
    pub published_at: String,
    pub asset: BundleAsset,
    /// True when the installed tag differs from the release tag.
    pub update_available: bool,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: Option<String>,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    assets: Option<Vec<GithubAsset>>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: Option<String>,
    size: Option<u64>,
    browser_download_url: Option<String>,
}

const USER_AGENT: &str = "McLauncher";
const ZIP_SUFFIX: &str = ".zip";

fn validate_repo(repo: &str) -> Result<&str, String> {
    let trimmed = repo.trim();
    let valid = !trimmed.is_empty()
        && trimmed.len() <= 200
        && trimmed.matches('/').count() == 1
        && trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/'));
    if !valid {
        return Err("Repository must look like \"owner/repo\".".to_string());
    }
    Ok(trimmed)
}

fn http_get(url: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(404, _) => "No such release was found on GitHub.".to_string(),
            ureq::Error::Status(403, _) => "GitHub rate limit reached. Try again in a few minutes.".to_string(),
            ureq::Error::Status(code, _) => format!("GitHub responded with HTTP {code}."),
            other => format!("Network error: {other}"),
        })?;
    response.into_string().map_err(|error| format!("Could not read the GitHub response: {error}"))
}

/// Asks GitHub for the newest release and compares it with the installed tag.
pub fn check(repo: &str, tag: &str, installed_tag: &str) -> Result<UpdateInfo, String> {
    let repo = validate_repo(repo)?;
    let url = if tag.trim().is_empty() {
        format!("https://api.github.com/repos/{repo}/releases/latest")
    } else {
        format!("https://api.github.com/repos/{repo}/releases/tags/{}", tag.trim())
    };
    let body = http_get(&url)?;
    let release: GithubRelease = serde_json::from_str(&body).map_err(|error| format!("Unexpected GitHub payload: {error}"))?;
    let release_tag = release.tag_name.clone().unwrap_or_else(|| tag.trim().to_string());
    let assets = release.assets.unwrap_or_default();
    let asset = assets
        .into_iter()
        .find(|item| {
            item.name
                .as_deref()
                .is_some_and(|name| name.to_ascii_lowercase().ends_with(ZIP_SUFFIX))
        })
        .ok_or_else(|| format!("Release {release_tag} has no .zip asset to download."))?;

    let name = asset.name.unwrap_or_default();
    let download_url = asset
        .browser_download_url
        .ok_or_else(|| "The release asset has no download URL.".to_string())?;
    let update_available = installed_tag.trim().is_empty() || installed_tag.trim() != release_tag;

    Ok(UpdateInfo {
        repo: repo.to_string(),
        tag: release_tag,
        name: release.name.unwrap_or_default(),
        notes: release.body.unwrap_or_default(),
        published_at: release.published_at.unwrap_or_default(),
        asset: BundleAsset { name, size: asset.size.unwrap_or(0), url: download_url },
        update_available,
    })
}

/// Streams an asset to `destination`, reporting bytes written through `on_progress`.
pub fn download<F>(url: &str, destination: &Path, expected_size: u64, mut on_progress: F) -> Result<(), String>
where
    F: FnMut(u64, u64),
{
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("Download failed: {error}"))?;

    let total = expected_size.max(1);
    let mut reader = response.into_reader();
    let mut file = File::create(destination)
        .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut written = 0u64;
    let mut last_report = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|error| format!("Download interrupted: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| format!("Could not write {}: {error}", destination.display()))?;
        written += read as u64;
        if written - last_report >= 512 * 1024 {
            on_progress(written, total);
            last_report = written;
        }
    }
    file.flush().map_err(|error| format!("Could not flush the download: {error}"))?;
    on_progress(written, total.max(written));
    Ok(())
}

/// Extracts a ZIP into `destination`, rejecting absolute and traversal paths.
pub fn extract_zip(archive: &Path, destination: &Path) -> Result<usize, String> {
    let file = File::open(archive).map_err(|error| format!("Could not open {}: {error}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| format!("Not a valid ZIP archive: {error}"))?;
    fs::create_dir_all(destination).map_err(|error| format!("Could not create {}: {error}", destination.display()))?;

    let mut extracted = 0usize;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("Could not read ZIP entry #{index}: {error}"))?;
        let entry_name = entry.name().to_string();
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("The archive contains an unsafe path: {entry_name}"))?;
        if enclosed.as_os_str().is_empty() {
            continue;
        }

        let output_path = destination.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|error| format!("Could not create {}: {error}", output_path.display()))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        }
        let mut output = File::create(&output_path)
            .map_err(|error| format!("Could not create {}: {error}", output_path.display()))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| format!("Could not extract {}: {error}", output_path.display()))?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err("The archive did not contain any files.".to_string());
    }
    Ok(extracted)
}

/// Archives that wrap everything in one folder are unwrapped so the game root is predictable.
pub fn unwrap_single_folder(root: &Path) -> Result<PathBuf, String> {
    let entries: Vec<PathBuf> = fs::read_dir(root)
        .map_err(|error| format!("Could not read {}: {error}", root.display()))?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    if entries.len() == 1 && entries[0].is_dir() {
        Ok(entries[0].clone())
    } else {
        Ok(root.to_path_buf())
    }
}

/// Moves the contents of `source` into `destination`, overwriting existing files.
pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("Could not read {}: {error}", source.display()))?
        .flatten()
    {
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else if from.is_file() {
            if to.exists() {
                let _ = fs::remove_file(&to);
            }
            fs::copy(&from, &to).map_err(|error| {
                format!("Could not copy {} to {}: {error}", from.display(), to.display())
            })?;
        }
    }
    Ok(())
}

/// Installs a downloaded archive: extract, unwrap, copy over the target directory.
pub fn install_from_archive(archive: &Path, staging: &Path, target: &Path) -> Result<PathBuf, String> {
    let extract_dir = staging.join("extract");
    let _ = fs::remove_dir_all(&extract_dir);
    extract_zip(archive, &extract_dir)?;
    let content_root = unwrap_single_folder(&extract_dir)?;
    fs::create_dir_all(target).map_err(|error| format!("Could not create {}: {error}", target.display()))?;
    copy_tree(&content_root, target)?;
    Ok(target.to_path_buf())
}
