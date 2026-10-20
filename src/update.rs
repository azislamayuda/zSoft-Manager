use crate::{
    config::{self, Source, SourceProvider},
    naming,
};
use serde::Deserialize;
use std::{
    cmp::Ordering,
    fs,
    io::Read,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub asset_name: String,
    pub url: String,
    pub sha256: Option<String>,
    pub size: u64,
    pub stable_filename: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Available,
    UpToDate,
    Uncertain,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
    size: u64,
}

pub fn check(
    source: &Source,
    original: &Path,
    saved_tag: Option<&str>,
) -> Result<(ReleaseInfo, CheckState), String> {
    if source.provider == SourceProvider::Github && !config::valid_repository(&source.repository) {
        return Err("Repository must be owner/repo".into());
    }
    let stable_filename = source
        .stable_filename
        .clone()
        .unwrap_or_else(|| naming::suggested_stable_filename(original));
    if !naming::valid_stable_filename(&stable_filename) {
        return Err("Invalid stable filename: use name.AppImage without a path".into());
    }
    let info = match source.provider {
        SourceProvider::Github => check_github(source, stable_filename)?,
        SourceProvider::KdeKdenlive => {
            if std::env::consts::ARCH != "x86_64" {
                return Err("Official KDE Kdenlive AppImage provider requires x86_64 Linux".into());
            }
            if !config::is_kdenlive_filename(
                original.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            ) {
                return Err("KDE Kdenlive provider only supports Kdenlive AppImages".into());
            }
            check_kde_kdenlive(source, stable_filename)?
        }
    };
    let from = saved_tag.and_then(version).or_else(|| {
        original
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(version)
    });
    let to = version(&info.tag);
    let state = match (from, to) {
        (Some(old), Some(new)) => match cmp_version(&old, &new) {
            Ordering::Less => CheckState::Available,
            Ordering::Equal | Ordering::Greater => CheckState::UpToDate,
        },
        _ if saved_tag == Some(info.tag.as_str()) => CheckState::UpToDate,
        _ => CheckState::Uncertain,
    };
    Ok((info, state))
}

const KDE_ROOT: &str = "https://download.kde.org/stable/kdenlive/";

fn fetch_kde_index(url: &str) -> Result<String, String> {
    // Never invoke a shell; bound the response and forbid protocols other than HTTPS.
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-redirs",
            "5",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "35",
            "--max-filesize",
            "2097152",
            "--header",
            "User-Agent: zSoft-Manager/0.3.0",
        ])
        .arg(url)
        .output()
        .map_err(|e| format!("Could not run curl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "KDE directory listing unavailable: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("KDE directory listing is not UTF-8: {e}"))
}

/// Strictly extract quoted HTML href values; never use untrusted hrefs as download URLs.
fn hrefs(html: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("href=") {
        rest = &rest[at + 5..];
        let Some(quote) = rest.chars().next() else {
            break;
        };
        if quote != '\'' && quote != '"' {
            continue;
        }
        rest = &rest[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        found.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    found
}

/// Directory segment like `26.08/`; rejects traversal, absolute URLs, and query strings.
fn kde_series(html: &str) -> Vec<(Vec<u64>, String)> {
    let mut result = Vec::new();
    for href in hrefs(html) {
        let Some(segment) = href.strip_suffix('/') else {
            continue;
        };
        let parts: Vec<_> = segment.split('.').collect();
        if parts.len() != 2
            || parts
                .iter()
                .any(|s| s.len() != 2 || !s.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let nums: Vec<_> = parts.iter().map(|s| s.parse::<u64>().unwrap()).collect();
        result.push((nums, segment.to_owned()));
    }
    result.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    result.dedup_by(|a, b| a.0 == b.0);
    result
}

/// Only stable Linux x86_64 Kdenlive AppImages; never nightly or alternative builds.
fn kde_release_assets(html: &str, asset_pattern: &str) -> Vec<(Vec<u64>, String)> {
    let mut result = Vec::new();
    for href in hrefs(html) {
        let Some(numeric) = href
            .strip_prefix("kdenlive-")
            .and_then(|s| s.strip_suffix("-x86_64.AppImage"))
        else {
            continue;
        };
        let parts: Vec<_> = numeric.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit())
            })
        {
            continue;
        }
        if !config::wildmatch(asset_pattern, href) {
            continue;
        }
        let nums: Vec<_> = parts.iter().map(|s| s.parse::<u64>().unwrap()).collect();
        result.push((nums, href.to_owned()));
    }
    result.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    result
}

/// Reject missing final HTTP Content-Length: installed AppImage must match bytes reported by server.
fn final_content_length(headers: &str) -> Option<u64> {
    let normalized = headers.replace('\r', "");
    let last = normalized
        .split("\n\n")
        .filter(|block| block.starts_with("HTTP/"))
        .last()?;
    if !last
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .starts_with('2')
    {
        return None;
    }
    last.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<u64>().ok())
                .flatten()
        })
        .filter(|size| *size > 0)
}

fn remote_kde_size(url: &str) -> Result<u64, String> {
    let output = Command::new("curl")
        .args([
            "--head",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-redirs",
            "5",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "45",
            "--header",
            "User-Agent: zSoft-Manager/0.3.0",
        ])
        .arg(url)
        .output()
        .map_err(|e| format!("Could not run curl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "KDE file headers unavailable: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    final_content_length(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
        "KDE mirror did not provide an exact file size; update canceled safely".into()
    })
}

fn check_kde_kdenlive(source: &Source, stable_filename: String) -> Result<ReleaseInfo, String> {
    let index = fetch_kde_index(KDE_ROOT)?;
    let series = kde_series(&index);
    if series.is_empty() {
        return Err("No KDE stable Kdenlive release series found".into());
    }
    // Try the newest release series first, including newly introduced ones.
    for (_, series_name) in series.into_iter().take(8) {
        let directory = format!("{KDE_ROOT}{series_name}/linux/");
        let Ok(html) = fetch_kde_index(&directory) else {
            continue;
        };
        let Some((version_parts, asset_name)) = kde_release_assets(&html, &source.asset_pattern)
            .into_iter()
            .next()
        else {
            continue;
        };
        let expected_series = format!("{}.{:02}", version_parts[0], version_parts[1]);
        if series_name != expected_series {
            continue;
        }
        let url = format!("{directory}{asset_name}");
        let size = remote_kde_size(&url)?;
        let tag = asset_name
            .strip_prefix("kdenlive-")
            .and_then(|name| name.strip_suffix("-x86_64.AppImage"))
            .ok_or("Invalid KDE AppImage filename")?
            .to_owned();
        return Ok(ReleaseInfo {
            tag,
            asset_name,
            url,
            sha256: None,
            size,
            stable_filename,
        });
    }
    Err("No stable Kdenlive x86_64 AppImage found on KDE's official server".into())
}

fn check_github(source: &Source, stable_filename: String) -> Result<ReleaseInfo, String> {
    let api = format!(
        "https://api.github.com/repos/{}/releases/latest",
        source.repository
    );
    let out = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "30",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "User-Agent: zSoft-Manager/0.3.0",
            &api,
        ])
        .output()
        .map_err(|e| format!("Cannot run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "GitHub API request failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let release: Release = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Invalid GitHub release response: {e}"))?;
    let candidates: Vec<_> = release
        .assets
        .iter()
        .filter(|a| {
            a.name.to_ascii_lowercase().ends_with(".appimage")
                && config::wildmatch(&source.asset_pattern, &a.name)
        })
        .collect();
    if candidates.len() != 1 {
        return Err(format!(
            "Expected one AppImage asset matching '{}'; found {}. Adjust the asset pattern.",
            source.asset_pattern,
            candidates.len()
        ));
    }
    let asset = candidates[0];
    let prefix = format!(
        "https://github.com/{}/releases/download/",
        source.repository
    );
    if !asset
        .browser_download_url
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
    {
        return Err("Release asset URL is not within the configured GitHub repository".into());
    }
    if asset.size == 0 {
        return Err("GitHub reports an empty release asset".into());
    }
    let sha256 = match asset.digest.as_deref() {
        None => None,
        Some(s) => {
            let d = s
                .strip_prefix("sha256:")
                .ok_or("Unsupported asset checksum format")?;
            if d.len() != 64 || !d.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Invalid GitHub SHA256 digest".into());
            }
            Some(d.to_ascii_lowercase())
        }
    };
    let info = ReleaseInfo {
        tag: release.tag_name,
        asset_name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        sha256,
        size: asset.size,
        stable_filename,
    };
    Ok(info)
}

// Conservative dotted-numeric versions. Unknown release schemes need manual one-at-a-time updating.
fn version(text: &str) -> Option<Vec<u64>> {
    let mut best: Option<Vec<u64>> = None;
    let mut part = String::new();
    let mut candidates: Vec<String> = Vec::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || (c == '.' && !part.is_empty()) {
            part.push(c);
        } else if !part.is_empty() {
            candidates.push(std::mem::take(&mut part));
        }
    }
    for candidate in candidates {
        let trimmed = candidate.trim_end_matches('.');
        if !trimmed.contains('.') {
            continue;
        }
        let nums: Option<Vec<u64>> = trimmed.split('.').map(|s| s.parse().ok()).collect();
        if let Some(nums) = nums {
            if nums.len() >= 2 && best.as_ref().is_none_or(|b| nums.len() > b.len()) {
                best = Some(nums);
            }
        }
    }
    best
}

fn cmp_version(a: &[u64], b: &[u64]) -> Ordering {
    for i in 0..a.len().max(b.len()) {
        match a
            .get(i)
            .copied()
            .unwrap_or(0)
            .cmp(&b.get(i).copied().unwrap_or(0))
        {
            Ordering::Equal => (),
            other => return other,
        }
    }
    Ordering::Equal
}

pub fn backup_for(original: &Path) -> PathBuf {
    let name = original
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(""))
        .to_string_lossy();
    original.with_file_name(format!(".{name}.zappimage-manager.backup"))
}

fn exists_even_if_broken_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Prevent updates from crossing out of the managed subdirectories (including symlinks).
fn managed_file(original: &Path) -> Result<(), String> {
    let root = fs::canonicalize(config::applications())
        .map_err(|e| format!("Applications folder: {e}"))?;
    let parent = fs::canonicalize(original.parent().ok_or("Invalid AppImage path")?)
        .map_err(|e| format!("AppImage folder: {e}"))?;
    if parent == root
        || !parent.starts_with(&root)
        || parent.strip_prefix(&root).unwrap().components().any(|c| {
            c.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("ArtCraft")
        })
    {
        return Err("AppImage is outside managed subfolders or inside ArtCraft".into());
    }
    let metadata = fs::symlink_metadata(original).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("AppImage must be a regular file (not symlink)".into());
    }
    if original
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|s| !s.eq_ignore_ascii_case("AppImage"))
    {
        return Err("Not an AppImage filename".into());
    }
    Ok(())
}

fn persist_source_mapping(original: &Path, stable: &str) -> Result<(), String> {
    let dir = original
        .parent()
        .and_then(|p| p.strip_prefix(config::applications()).ok())
        .ok_or("Unable to identify application directory")?
        .to_string_lossy()
        .to_string();
    let old_name = original
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid AppImage filename")?;
    config::ensure_stable_source(&dir, old_name, stable)
}

/// Save an already-verified downloaded binary. In both branches, the backup basename is stable.
/// A legacy name is kept as a lightweight symlink for existing desktop shortcuts.
fn activate_verified(original: &Path, target: &Path, ready: &Path) -> Result<(), String> {
    if !exists_even_if_broken_link(ready) {
        return Err("Verified download is missing".into());
    }
    let backup = backup_for(target);
    if target != original {
        if exists_even_if_broken_link(target) {
            return Err(format!("Target already exists: {}", target.display()));
        }
        if exists_even_if_broken_link(&backup) {
            return Err(format!(
                "Backup target already exists: {}",
                backup.display()
            ));
        }
        fs::rename(original, &backup).map_err(|e| format!("Cannot make backup: {e}"))?;
        if let Err(e) = fs::rename(ready, target) {
            let restore = fs::rename(&backup, original);
            return Err(format!(
                "Cannot install stable file: {e}; restore: {restore:?}"
            ));
        }
        let target_name = target.file_name().ok_or("Missing target filename")?;
        if let Err(e) = symlink(target_name, original) {
            // Restore the exact previous state if we cannot preserve legacy shortcuts.
            let put_back = fs::rename(target, ready);
            let restore = fs::rename(&backup, original);
            return Err(format!("Cannot create shortcut-compatible symlink: {e}; new file move: {put_back:?}; backup restore: {restore:?}"));
        }
        return Ok(());
    }
    // Replacing an already stable name: preserve one prior backup during the whole swap.
    let rotation = backup.with_file_name(format!(
        "{}.rotate-{}",
        backup.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    if exists_even_if_broken_link(&rotation) {
        return Err("Temporary backup rotation already exists".into());
    }
    if exists_even_if_broken_link(&backup) {
        if !fs::symlink_metadata(&backup)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_file()
        {
            return Err("Existing backup is not a regular file".into());
        }
        fs::rename(&backup, &rotation).map_err(|e| format!("Cannot rotate backup: {e}"))?;
    }
    if let Err(e) = fs::rename(original, &backup) {
        if exists_even_if_broken_link(&rotation) {
            let _ = fs::rename(&rotation, &backup);
        }
        return Err(format!("Cannot back up current AppImage: {e}"));
    }
    if let Err(e) = fs::rename(ready, target) {
        let restore = fs::rename(&backup, original);
        if exists_even_if_broken_link(&rotation) {
            let _ = fs::rename(&rotation, &backup);
        }
        return Err(format!("Cannot install update: {e}; restore: {restore:?}"));
    }
    if exists_even_if_broken_link(&rotation) {
        let _ = fs::remove_file(&rotation);
    }
    Ok(())
}

/// Only touch Exec/TryExec values, not unrelated Desktop Entry keys.
fn rewrite_desktop_entry(content: &str, from: &str, to: &str) -> Option<String> {
    let mut replacement = String::with_capacity(content.len());
    let mut changed = false;
    for line in content.split_inclusive('\n') {
        if (line.starts_with("Exec=") || line.starts_with("TryExec=")) && line.contains(from) {
            replacement.push_str(&line.replace(from, to));
            changed = true;
        } else {
            replacement.push_str(line);
        }
    }
    changed.then_some(replacement)
}

/// Update only the invoking user's launchers, and only Exec/TryExec references to the exact old path.
/// Existing legacy symlink remains as a fallback for other launchers.
fn repair_user_launchers(original: &Path, target: &Path) -> usize {
    if original == target {
        return 0;
    }
    let directory = config::home().join(".local/share/applications");
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    let Some(from) = original.to_str() else {
        return 0;
    };
    let Some(to) = target.to_str() else {
        return 0;
    };
    let mut fixed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(replacement) = rewrite_desktop_entry(&content, from, to) else {
            continue;
        };
        let temp = path.with_extension(format!("desktop.zappimage-{}.tmp", std::process::id()));
        if exists_even_if_broken_link(&temp) {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if fs::write(&temp, &replacement).is_ok()
            && fs::set_permissions(&temp, meta.permissions()).is_ok()
            && fs::rename(&temp, &path).is_ok()
        {
            fixed += 1;
        }
        let _ = fs::remove_file(&temp);
    }
    fixed
}

fn move_tracking(old: &Path, target: &Path, new_tag: Option<&str>) -> Result<(), String> {
    let mut state = config::load_installed()?;
    let old_key = old.to_string_lossy().to_string();
    let new_key = target.to_string_lossy().to_string();
    let previous = state.tags.remove(&old_key).or_else(|| {
        old.file_name()
            .and_then(|s| s.to_str())
            .and_then(version)
            .map(|parts| {
                parts
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(".")
            })
    });
    if old != target {
        if let Some(tag) = state.previous_tags.remove(&old_key) {
            state.previous_tags.insert(new_key.clone(), tag);
        }
    }
    if let Some(tag) = new_tag {
        if let Some(old_tag) = previous {
            state.previous_tags.insert(new_key.clone(), old_tag);
        } else {
            state.previous_tags.remove(&new_key);
        }
        state.tags.insert(new_key, tag.to_owned());
    } else if let Some(old_tag) = previous {
        state.tags.insert(new_key, old_tag);
    }
    config::save_installed(&state)
}

pub fn install(original: &Path, info: &ReleaseInfo) -> Result<String, String> {
    managed_file(original)?;
    if !naming::valid_stable_filename(&info.stable_filename) {
        return Err("Invalid stable AppImage filename".into());
    }
    let target = original.with_file_name(&info.stable_filename);
    if target != original && exists_even_if_broken_link(&target) {
        return Err(format!(
            "Stable filename is already used: {}",
            target.display()
        ));
    }
    let metadata = fs::symlink_metadata(original).map_err(|e| e.to_string())?;
    let folder = original.parent().ok_or("Invalid original path")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temporary = folder.join(format!(
        ".zappimage-download-{}-{now}.part",
        std::process::id()
    ));
    let result = (|| -> Result<String, String> {
        let output = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--retry",
                "2",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--connect-timeout",
                "15",
                "--max-time",
                "900",
                "--output",
            ])
            .arg(&temporary)
            .arg(&info.url)
            .output()
            .map_err(|e| format!("Cannot run curl: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "Download failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let actual_size = fs::metadata(&temporary).map_err(|e| e.to_string())?.len();
        if actual_size != info.size {
            return Err(format!(
                "Downloaded size mismatch: expected {}, got {}",
                info.size, actual_size
            ));
        }
        let mut file = fs::File::open(&temporary).map_err(|e| e.to_string())?;
        let mut elf_magic = [0u8; 4];
        file.read_exact(&mut elf_magic).map_err(|e| e.to_string())?;
        if &elf_magic != b"\x7fELF" {
            return Err("File is not an ELF AppImage".into());
        }
        if let Some(expected) = &info.sha256 {
            let out = Command::new("sha256sum")
                .arg(&temporary)
                .output()
                .map_err(|e| format!("Cannot verify SHA256: {e}"))?;
            if !out.status.success() {
                return Err("SHA256 verification failed".into());
            }
            let outstr = String::from_utf8_lossy(&out.stdout);
            let digest = outstr.split_whitespace().next().unwrap_or("");
            if !digest.eq_ignore_ascii_case(expected) {
                return Err("SHA256 mismatch; update cancelled".into());
            }
        }
        fs::set_permissions(
            &temporary,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        )
        .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        // Write the source alias first so this app remains identifiable after filename migration.
        persist_source_mapping(original, &info.stable_filename)?;
        activate_verified(original, &target, &temporary)?;
        let state_warning = move_tracking(original, &target, Some(&info.tag)).err();
        let repaired = repair_user_launchers(original, &target);
        let verified = if info.sha256.is_some() {
            "SHA256 verified"
        } else {
            "no publisher SHA256 provided"
        };
        let mut msg = format!(
            "Installed version {} as {} ({verified}); {} launchers repaired; rollback available",
            info.tag,
            target.file_name().unwrap().to_string_lossy(),
            repaired
        );
        if let Some(e) = state_warning {
            msg.push_str(&format!("; version tracking warning: {e}"));
        }
        Ok(msg)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

/// User-requested rename without network/download; does not alter the application bytes.
/// Filesystem-only filename migration, allowing isolated tests without touching HOME.
fn move_to_stable(original: &Path, target: &Path) -> Result<(), String> {
    if exists_even_if_broken_link(target) {
        return Err(format!(
            "Target filename is already in use: {}",
            target.display()
        ));
    }
    fs::rename(original, target).map_err(|e| format!("Rename failed: {e}"))?;
    if let Err(e) = symlink(target.file_name().ok_or("Invalid target")?, original) {
        let restored = fs::rename(target, original);
        return Err(format!(
            "Shortcut compatibility symlink failed: {e}; restore: {restored:?}"
        ));
    }
    Ok(())
}

pub fn normalize(original: &Path, stable: &str) -> Result<String, String> {
    managed_file(original)?;
    if !naming::valid_stable_filename(stable) {
        return Err("Invalid stable filename".into());
    }
    let target = original.with_file_name(stable);
    if target == original {
        return Ok("AppImage already has a stable versionless filename".into());
    }
    if exists_even_if_broken_link(&target) {
        return Err(format!(
            "Target filename is already in use: {}",
            target.display()
        ));
    }
    persist_source_mapping(original, stable)?;
    move_to_stable(original, &target)?;
    let old_backup = backup_for(original);
    let new_backup = backup_for(&target);
    if exists_even_if_broken_link(&old_backup) && !exists_even_if_broken_link(&new_backup) {
        // Existing v0.1.0 backup remains restorable under the new stable filename.
        let _ = fs::rename(&old_backup, &new_backup);
    }
    let warning = move_tracking(original, &target, None).err();
    let repaired = repair_user_launchers(original, &target);
    let mut msg = format!(
        "Renamed to {stable}; {repaired} launchers repaired; compatibility symlink created"
    );
    if let Some(e) = warning {
        msg.push_str(&format!("; version tracking warning: {e}"));
    }
    Ok(msg)
}

/// Restore the prior binary at the *same stable path*, without renaming it to a versioned name.
fn rollback_files(original: &Path) -> Result<(), String> {
    let backup = backup_for(original);
    if !fs::symlink_metadata(&backup)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_file()
    {
        return Err("Backup is not a regular file".into());
    }
    let swapped =
        original.with_file_name(format!(".zappimage-rollback-{}.part", std::process::id()));
    if exists_even_if_broken_link(&swapped) {
        return Err("Temporary rollback file already exists".into());
    }
    fs::rename(original, &swapped).map_err(|e| e.to_string())?;
    if let Err(e) = fs::rename(&backup, original) {
        let restored = fs::rename(&swapped, original);
        return Err(format!(
            "Rollback failed: {e}; original restoration: {restored:?}"
        ));
    }
    let _ = fs::remove_file(swapped);
    Ok(())
}

pub fn rollback(original: &Path) -> Result<(), String> {
    managed_file(original)?;
    rollback_files(original)?;
    let mut state = config::load_installed()?;
    let key = original.to_string_lossy().to_string();
    match state.previous_tags.remove(&key) {
        Some(previous) => {
            state.tags.insert(key, previous);
        }
        None => {
            state.tags.remove(&key);
        }
    }
    config::save_installed(&state)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zappimage-v02-{}-{now}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn kde_series_ignores_parent_and_untrusted_links() {
        let html = r#"<a href="../">Parent</a><a href="26.08/">26.08</a><a href="26.12/">26.12</a><a href="https://evil.test/99.99/">evil</a><a href="26.08/">same</a>"#;
        let series = kde_series(html);
        assert_eq!(
            series.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(),
            vec!["26.12", "26.08"]
        );
    }
    #[test]
    fn kde_assets_select_latest_stable_linux_amd64() {
        let html = r#"<a href="kdenlive-26.08.0-x86_64.AppImage">0</a><a href="kdenlive-26.08.2-x86_64.AppImage">2</a><a href="kdenlive-26.08.11-x86_64.AppImage">11</a><a href="kdenlive-26.08.99-aarch64.AppImage">arm</a><a href="kdenlive-nightly-x86_64.AppImage">nightly</a><a href="../kdenlive-99.99.99-x86_64.AppImage">traversal</a>"#;
        let assets = kde_release_assets(html, "kdenlive-*-x86_64.AppImage");
        assert_eq!(assets[0].1, "kdenlive-26.08.11-x86_64.AppImage");
        assert_eq!(assets.len(), 3);
    }
    #[test]
    fn final_size_requires_last_successful_http_response() {
        let h = "HTTP/1.1 302 Found\r\nContent-Length: 3\r\n\r\nHTTP/2 200\r\ncontent-length: 214748365\r\n\r\n";
        assert_eq!(final_content_length(h), Some(214748365));
        assert_eq!(
            final_content_length("HTTP/2 404\r\nContent-Length: 99\r\n"),
            None
        );
        assert_eq!(
            final_content_length("HTTP/2 200\r\ncontent-type: text/html\r\n"),
            None
        );
    }
    #[test]
    fn versions() {
        assert_eq!(
            version("drawio-x86_64-32.5.5.AppImage"),
            Some(vec![32, 5, 5])
        );
        assert_eq!(version("v1.12.0"), Some(vec![1, 12, 0]));
        assert_eq!(cmp_version(&[1, 8, 0], &[1, 10, 0]), Ordering::Less);
        assert_eq!(cmp_version(&[2, 0], &[2, 0, 0]), Ordering::Equal);
        assert_eq!(version("continuous"), None);
    }
    #[test]
    fn first_migration_retains_backup_and_shortcut_compatibility() {
        let dir = fixture();
        let old = dir.join("drawio-x86_64-32.4.1.AppImage");
        let stable = dir.join("drawio.AppImage");
        let ready = dir.join("release.part");
        fs::write(&old, "old").unwrap();
        fs::write(&ready, "new").unwrap();
        activate_verified(&old, &stable, &ready).unwrap();
        assert_eq!(fs::read(&stable).unwrap(), b"new");
        assert_eq!(fs::read(backup_for(&stable)).unwrap(), b"old");
        assert!(fs::symlink_metadata(&old).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&old).unwrap(), b"new");
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn subsequent_update_preserves_stable_filename_and_latest_backup() {
        let dir = fixture();
        let stable = dir.join("drawio.AppImage");
        let ready1 = dir.join("one.part");
        let ready2 = dir.join("two.part");
        fs::write(&stable, "v1").unwrap();
        fs::write(&ready1, "v2").unwrap();
        activate_verified(&stable, &stable, &ready1).unwrap();
        assert_eq!(fs::read(backup_for(&stable)).unwrap(), b"v1");
        fs::write(&ready2, "v3").unwrap();
        activate_verified(&stable, &stable, &ready2).unwrap();
        assert_eq!(fs::read(&stable).unwrap(), b"v3");
        assert_eq!(fs::read(backup_for(&stable)).unwrap(), b"v2");
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn only_executable_paths_are_updated_in_user_desktop_entries() {
        let input = "[Desktop Entry]\nExec=\"/home/user/Applications/drawio/drawio-1.0.AppImage\" %F\nIcon=/home/user/Applications/drawio/drawio-1.0.AppImage\nTryExec=/home/user/Applications/drawio/drawio-1.0.AppImage\n";
        let output = rewrite_desktop_entry(
            input,
            "/home/user/Applications/drawio/drawio-1.0.AppImage",
            "/home/user/Applications/drawio/drawio.AppImage",
        )
        .unwrap();
        assert!(output.contains("Exec=\"/home/user/Applications/drawio/drawio.AppImage\" %F"));
        assert!(output.contains("TryExec=/home/user/Applications/drawio/drawio.AppImage"));
        assert!(output.contains("Icon=/home/user/Applications/drawio/drawio-1.0.AppImage"));
    }
    #[test]
    fn normalize_renames_but_does_not_modify_binary() {
        let dir = fixture();
        let old = dir.join("Whazapp_0.1.5.1_amd64.AppImage");
        let stable = dir.join("Whazapp.AppImage");
        fs::write(&old, b"\x7fELF-original").unwrap();
        move_to_stable(&old, &stable).unwrap();
        assert_eq!(fs::read(&stable).unwrap(), b"\x7fELF-original");
        assert_eq!(fs::read(&old).unwrap(), b"\x7fELF-original");
        assert!(fs::symlink_metadata(&old).unwrap().file_type().is_symlink());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rollback_keeps_stable_name_and_old_shortcut() {
        let dir = fixture();
        let old = dir.join("drawio-1.0.AppImage");
        let stable = dir.join("drawio.AppImage");
        let ready = dir.join("new.part");
        fs::write(&old, "old-version").unwrap();
        fs::write(&ready, "new-version").unwrap();
        activate_verified(&old, &stable, &ready).unwrap();
        rollback_files(&stable).unwrap();
        assert_eq!(fs::read(&stable).unwrap(), b"old-version");
        assert_eq!(fs::read(&old).unwrap(), b"old-version");
        assert!(!exists_even_if_broken_link(&backup_for(&stable)));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn refusing_existing_stable_target_does_not_overwrite_files() {
        let dir = fixture();
        let old = dir.join("drawio-1.2.AppImage");
        let target = dir.join("drawio.AppImage");
        let ready = dir.join("download.part");
        fs::write(&old, "old").unwrap();
        fs::write(&target, "other").unwrap();
        fs::write(&ready, "download").unwrap();
        assert!(activate_verified(&old, &target, &ready).is_err());
        assert_eq!(fs::read(&old).unwrap(), b"old");
        assert_eq!(fs::read(&target).unwrap(), b"other");
        fs::remove_dir_all(&dir).unwrap();
    }
}
