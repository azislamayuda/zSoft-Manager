use crate::{
    config::{self, Source, SourceProvider},
    naming,
};
use serde::{de::DeserializeOwned, Deserialize};
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
    UserConfirmed,
    Uncertain,
}

/// A release can be public before its AppImage and checksum are uploaded.
/// This is an expected transient state, not a wrong configuration.
#[derive(Debug)]
pub enum CheckOutcome {
    Ready(ReleaseInfo, CheckState),
    Pending { tag: String, reason: String },
}

enum GithubResponse {
    Ready(ReleaseInfo),
    Pending { tag: String, reason: String },
}

enum AssetPick<'a> {
    Ready(&'a Asset),
    Pending(&'static str),
    NoAppImage,
}

/// The caller can distinguish a wrong pattern from an unsupported architecture
/// without parsing an English error message. Neither is an upload-in-progress.
#[derive(Debug)]
enum AssetSelectionError {
    WrongArchitecture(String),
    PatternMismatch { pattern: String, available: String },
    Ambiguous { pattern: String, matching: String },
}

impl std::fmt::Display for AssetSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongArchitecture(available) => write!(f,
                "Architecture mismatch: no x86_64 AppImage; available AppImages: {available}"),
            Self::PatternMismatch { pattern, available } => write!(f,
                "Asset pattern mismatch: '{pattern}' does not match a compatible AppImage. Available: {available}. Choose a suitable pattern."),
            Self::Ambiguous { pattern, matching } => write!(f,
                "Ambiguous AppImage assets matching '{pattern}': {matching}. Specify a narrower asset pattern."),
        }
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    #[serde(default)]
    state: Option<String>,
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
    match check_with_readiness(source, original, saved_tag, false)? {
        CheckOutcome::Ready(info, state) => Ok((info, state)),
        CheckOutcome::Pending { tag, reason } => Err(format!(
            "Release {tag} is published but not ready: {reason}. Try again later."
        )),
    }
}

/// A self-update must have a verified SHA256 (GitHub digest or published sidecar).
/// Other AppImage providers retain their previous compatibility behavior.
pub fn check_with_readiness(
    source: &Source,
    original: &Path,
    saved_tag: Option<&str>,
    require_checksum: bool,
) -> Result<CheckOutcome, String> {
    if source.provider == SourceProvider::Github {
        if !config::valid_repository(&source.repository) {
            return Err("Repository must be owner/repo".into());
        }
        if std::env::consts::ARCH != "x86_64" {
            return Err("GitHub AppImage auto-detection currently supports x86_64 only".into());
        }
    }
    let stable_filename = source
        .stable_filename
        .clone()
        .unwrap_or_else(|| naming::suggested_stable_filename(original));
    if !naming::valid_stable_filename(&stable_filename) {
        return Err("Invalid stable filename: use name.AppImage without a path".into());
    }
    let info = match source.provider {
        SourceProvider::Github => match check_github(source, stable_filename, require_checksum)? {
            GithubResponse::Ready(info) => info,
            GithubResponse::Pending { tag, reason } => {
                return Ok(CheckOutcome::Pending { tag, reason })
            }
        },
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
    let state = installed_check_state_for_source(original, saved_tag, &info, source);
    Ok(CheckOutcome::Ready(info, state))
}

/// Hash a local AppImage without executing it, mounting it or invoking a shell.
/// This is called only by an explicit user action in a worker thread.
fn local_sha256(path: &Path) -> Result<String, String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("Local AppImage unavailable: {e}"))?;
    if !meta.file_type().is_file() || meta.len() == 0 {
        return Err("Local AppImage must be a non-empty regular file".into());
    }
    let output = Command::new("sha256sum")
        .arg("--")
        .arg(path)
        .output()
        .map_err(|e| format!("Cannot hash local AppImage: {e}"))?;
    if !output.status.success() {
        return Err("Could not calculate local AppImage SHA256".into());
    }
    let bytes = output
        .stdout
        .split(|byte| byte.is_ascii_whitespace())
        .next()
        .ok_or("Missing local SHA256 digest")?;
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid local SHA256 output")?;
    sha256_digest(text).ok_or_else(|| "Invalid local SHA256 digest".into())
}

/// Compare exact bytes with the release, even when the file has a stable name.
#[cfg(test)]
fn local_matches_release_checksum(original: &Path, info: &ReleaseInfo) -> bool {
    let Some(expected) = info.sha256.as_deref() else {
        return false;
    };
    if info.size == 0
        || !fs::metadata(original).is_ok_and(|meta| meta.is_file() && meta.len() == info.size)
    {
        return false;
    }
    local_sha256(original).is_ok_and(|digest| digest.eq_ignore_ascii_case(expected))
}

/// A manual tag is useful for renamed AppImages, but is not cryptographic
/// evidence of the upstream version. It is tied to the repository and local
/// file SHA256, and becomes invalid whenever the file is replaced.
fn confirmed_local_version(
    repository: &str,
    confirmation: Option<&config::LocalVersionConfirmation>,
    file_size: Option<u64>,
    computed_sha256: Option<&str>,
) -> Option<String> {
    let record = confirmation?;
    if repository.is_empty() || record.repository != repository || record.tag.is_empty() {
        return None;
    }
    if file_size != Some(record.file_size) {
        return None;
    }
    (computed_sha256 == Some(record.sha256.as_str())).then(|| record.tag.clone())
}

fn checked_state_with_confirmation(
    original: &Path,
    saved_tag: Option<&str>,
    info: &ReleaseInfo,
    repository: &str,
    confirmation: Option<&config::LocalVersionConfirmation>,
) -> CheckState {
    // Compute at most one local SHA256 even if publisher SHA256 is present
    // but different and a manual assertion also exists. Work is off the UI thread.
    let regular_file_size = fs::symlink_metadata(original)
        .ok()
        .and_then(|m| m.file_type().is_file().then_some(m.len()));
    let publisher_match_size =
        info.sha256.is_some() && info.size != 0 && regular_file_size == Some(info.size);
    let has_confirmation = confirmation.is_some_and(|record| {
        !repository.is_empty()
            && record.repository == repository
            && regular_file_size == Some(record.file_size)
    });
    let local_digest = if publisher_match_size || has_confirmation {
        local_sha256(original).ok()
    } else {
        None
    };
    if publisher_match_size && info.sha256.as_deref() == local_digest.as_deref() {
        return CheckState::UpToDate;
    }
    let manually_set = confirmed_local_version(
        repository,
        confirmation,
        regular_file_size,
        local_digest.as_deref(),
    );
    if manually_set.as_deref() == Some(info.tag.as_str()) {
        return CheckState::UserConfirmed;
    }
    // Existing zSoft-managed installations continue to use their saved tag.
    if saved_tag == Some(info.tag.as_str()) {
        return CheckState::UpToDate;
    }
    let source_is_user_confirmation = manually_set.is_some();
    let from = manually_set
        .as_deref()
        .and_then(version)
        .or_else(|| saved_tag.and_then(version))
        .or_else(|| {
            original
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(version)
        });
    let to = version(&info.tag);
    match (from, to) {
        (Some(old), Some(new)) => match cmp_version(&old, &new) {
            Ordering::Less => CheckState::Available,
            Ordering::Equal | Ordering::Greater => {
                if source_is_user_confirmation {
                    CheckState::UserConfirmed
                } else {
                    CheckState::UpToDate
                }
            }
        },
        _ => CheckState::Uncertain,
    }
}

#[cfg(test)]
fn installed_check_state(
    original: &Path,
    saved_tag: Option<&str>,
    info: &ReleaseInfo,
) -> CheckState {
    checked_state_with_confirmation(original, saved_tag, info, "", None)
}

fn installed_check_state_for_source(
    original: &Path,
    saved_tag: Option<&str>,
    info: &ReleaseInfo,
    source: &Source,
) -> CheckState {
    let key = original.to_string_lossy();
    let installed = config::load_installed().ok();
    let record = installed
        .as_ref()
        .and_then(|state| state.local_version_confirmations.get(key.as_ref()));
    checked_state_with_confirmation(original, saved_tag, info, &source.repository, record)
}

/// User explicitly states the installed version shown by that application's
/// About page. This does not replace/download the AppImage or claim an upstream
/// checksum match. The assertion remains valid only for the unchanged local
/// binary and repository.
pub fn confirm_installed_version(
    original: &Path,
    source: &Source,
    info: &ReleaseInfo,
    installed_tag: &str,
) -> Result<CheckState, String> {
    if source.provider != SourceProvider::Github || !config::valid_repository(&source.repository) {
        return Err("Manual version confirmation is available for GitHub sources only".into());
    }
    if !safe_asset_url(&source.repository, &info.url) {
        return Err("Latest release does not match the configured source".into());
    }
    let installed_tag = installed_tag.trim();
    if installed_tag.is_empty()
        || installed_tag.len() > 128
        || installed_tag.chars().any(char::is_control)
    {
        return Err("Enter a valid installed release tag (up to 128 characters)".into());
    }
    // Only managed AppImages; do not follow symlinks out of the Applications tree.
    managed_file(original)?;
    let size = fs::symlink_metadata(original)
        .map_err(|e| e.to_string())?
        .len();
    let digest = local_sha256(original)?;
    let mut state = config::load_installed()?;
    state.local_version_confirmations.insert(
        original.to_string_lossy().into_owned(),
        config::LocalVersionConfirmation {
            repository: source.repository.clone(),
            tag: installed_tag.into(),
            sha256: digest,
            file_size: size,
        },
    );
    config::save_installed(&state)?;
    Ok(installed_check_state_for_source(
        original, None, info, source,
    ))
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

/// GitHub projects use x86_64, x64, amd64, or no architecture in AppImage names.
/// Never guess that ARM/RISC-V/32-bit assets can run on this x86_64 installation.
fn compatible_x86_64_appimage(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".appimage") {
        return false;
    }
    ![
        "aarch64",
        "arm64",
        "arm32",
        "armv6",
        "armv7",
        "armv8",
        "armhf",
        "armel",
        "riscv",
        "ppc64",
        "s390",
        "loongarch",
        "mips",
        "i386",
        "i686",
        "x86-32",
        "x86_32",
        "-arm.",
        "_arm.",
        "-x86.appimage",
        "_x86.appimage",
    ]
    .iter()
    .any(|foreign| lower.contains(foreign))
}

/// Only the generic architecture template gets an automatic alias fallback.
/// User-specified product patterns must not accidentally select another product.
fn generic_x86_64_pattern(pattern: &str) -> bool {
    matches!(
        pattern.to_ascii_lowercase().as_str(),
        "*x86_64*.appimage" | "*x64*.appimage" | "*amd64*.appimage" | "*.appimage"
    )
}

/// The selection is fully testable offline with a small deserialized fixture.
fn select_asset<'a>(
    release: &'a Release,
    pattern: &str,
    require_checksum: bool,
) -> Result<AssetPick<'a>, AssetSelectionError> {
    let appimages: Vec<&Asset> = release
        .assets
        .iter()
        .filter(|asset| asset.name.to_ascii_lowercase().ends_with(".appimage"))
        .collect();
    if appimages.is_empty() {
        return if require_checksum {
            Ok(AssetPick::Pending(
                "The latest release has no AppImage asset yet",
            ))
        } else {
            Ok(AssetPick::NoAppImage)
        };
    }

    let compatible: Vec<&Asset> = appimages
        .iter()
        .copied()
        .filter(|asset| compatible_x86_64_appimage(&asset.name))
        .collect();
    if compatible.is_empty() {
        return Err(AssetSelectionError::WrongArchitecture(
            appimages
                .iter()
                .take(4)
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }

    // Respect specific configured patterns. Generic x86_64 patterns can accept
    // x64, amd64, or unlabelled AppImages only when the exact pattern fails.
    let exact: Vec<&Asset> = compatible
        .iter()
        .copied()
        .filter(|asset| config::wildmatch(pattern, &asset.name))
        .collect();
    let matches: Vec<&Asset> = if exact.is_empty() && generic_x86_64_pattern(pattern) {
        compatible.iter().copied().collect()
    } else {
        exact
    };
    if matches.is_empty() {
        return Err(AssetSelectionError::PatternMismatch {
            pattern: pattern.into(),
            available: compatible
                .iter()
                .take(4)
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    if matches.len() != 1 {
        return Err(AssetSelectionError::Ambiguous {
            pattern: pattern.into(),
            matching: matches
                .iter()
                .take(5)
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }

    let asset = matches[0];
    if asset.size == 0
        || asset
            .state
            .as_deref()
            .is_some_and(|state| state != "uploaded")
    {
        return Ok(AssetPick::Pending(
            "The selected AppImage upload is not complete",
        ));
    }
    if require_checksum && asset.digest.is_none() {
        let sidecar_name = format!("{}.sha256", asset.name);
        let sidecar = release
            .assets
            .iter()
            .find(|candidate| candidate.name == sidecar_name);
        if sidecar.is_none_or(|value| {
            value.size == 0
                || value
                    .state
                    .as_deref()
                    .is_some_and(|state| state != "uploaded")
        }) {
            return Ok(AssetPick::Pending(
                "The SHA256 checksum has not been uploaded yet",
            ));
        }
    }
    Ok(AssetPick::Ready(asset))
}

/// Find an older published release only when the latest one has no usable
/// desktop AppImage (e.g., the Obsidian repository's mobile-only releases).
fn find_compatible_release<'a>(
    releases: &'a [Release],
    latest_tag: &str,
    pattern: &str,
) -> Option<(&'a Release, &'a Asset)> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease && release.tag_name != latest_tag)
        .find_map(|release| match select_asset(release, pattern, false) {
            Ok(AssetPick::Ready(asset)) => Some((release, asset)),
            _ => None,
        })
}

fn sha256_digest(value: &str) -> Option<String> {
    let digest = value.strip_prefix("sha256:").unwrap_or(value);
    (digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| digest.to_ascii_lowercase())
}

/// Accept `HASH` or sha256sum's `HASH  filename`; reject other filenames.
fn parse_sha256_sidecar(text: &str, expected_filename: &str) -> Option<String> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let line = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    let mut columns = line.split_whitespace();
    let digest = sha256_digest(columns.next()?)?;
    if let Some(filename) = columns.next() {
        if filename.trim_start_matches('*') != expected_filename {
            return None;
        }
    }
    if columns.next().is_some() {
        return None;
    }
    Some(digest)
}

fn safe_asset_url(repo: &str, url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{repo}/releases/download/"))
}

fn download_sha256_sidecar(url: &str, expected_filename: &str) -> Result<String, String> {
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
            "20",
            "--max-filesize",
            "4096",
            "--header",
            "Cache-Control: no-cache",
        ])
        .arg(url)
        .output()
        .map_err(|e| format!("Could not check SHA256 asset: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "SHA256 asset is not downloadable yet: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let content =
        String::from_utf8(output.stdout).map_err(|_| "SHA256 asset must be UTF-8".to_string())?;
    parse_sha256_sidecar(&content, expected_filename)
        .ok_or_else(|| "The published SHA256 asset has invalid content".into())
}

// curl writes the final HTTP status after the JSON body, so errors are never
// reduced to an unexplained `curl: (22)` failure. No credentials are logged.
const GITHUB_HTTP_MARKER: &[u8] = b"\n__ZSOFT_GITHUB_HTTP__=";

fn split_github_http_response(raw: &[u8]) -> Result<(u16, &[u8]), String> {
    let pos = raw
        .windows(GITHUB_HTTP_MARKER.len())
        .rposition(|part| part == GITHUB_HTTP_MARKER)
        .ok_or("Missing GitHub HTTP status from curl")?;
    let status = std::str::from_utf8(&raw[pos + GITHUB_HTTP_MARKER.len()..])
        .map_err(|_| "Invalid GitHub HTTP status encoding")?
        .trim()
        .parse::<u16>()
        .map_err(|_| "Invalid GitHub HTTP status")?;
    Ok((status, &raw[..pos]))
}

fn github_http_error(status: u16, body: &[u8]) -> String {
    let message = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|data| {
            data.get("message")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default();
    // Do not place entire server-controlled error bodies in the GTK label.
    let detail: String = message
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(180)
        .collect();
    let lower = detail.to_ascii_lowercase();
    let rate_limited = status == 429
        || (status == 403 && (lower.contains("rate limit") || lower.contains("rate_limit")));
    let category = if rate_limited {
        "GitHub API rate limit exceeded"
    } else {
        match status {
            401 => "GitHub API authentication rejected",
            403 => "GitHub API access denied",
            404 => "GitHub repository or release not found",
            500..=599 => "GitHub API temporarily unavailable",
            _ => "GitHub API HTTP error",
        }
    };
    if rate_limited {
        return format!("{category} (HTTP {status}). Wait for the GitHub limit to reset before checking again. {detail}").trim().to_owned();
    }
    format!("{category} (HTTP {status}). {detail}")
        .trim()
        .to_owned()
}

fn fetch_github_json<T: DeserializeOwned>(api: &str, max_bytes: &str) -> Result<T, String> {
    let output = Command::new("curl")
        .args([
            "--fail-with-body",
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
            "30",
            "--max-filesize",
            max_bytes,
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "User-Agent: zSoft-Manager/0.4.7",
            "--write-out",
            "\\n__ZSOFT_GITHUB_HTTP__=%{http_code}\\n",
        ])
        .arg(api)
        .output()
        .map_err(|e| format!("Cannot run curl: {e}"))?;
    // curl --write-out uses backslash escapes. The HTTP status is printed even
    // when --fail-with-body returns exit 22 on a 4xx/5xx response.
    if !output.status.success() && output.status.code() != Some(22) {
        let kind = match output.status.code() {
            Some(6 | 7 | 28 | 35 | 60) => "Network error",
            Some(63) => "GitHub API response exceeds the safety size limit",
            _ => "GitHub API request error",
        };
        return Err(format!(
            "{kind}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let (status, body) = split_github_http_response(&output.stdout)?;
    if !(200..300).contains(&status) {
        return Err(github_http_error(status, body));
    }
    if !output.status.success() {
        return Err(format!(
            "GitHub API response could not be received: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(body).map_err(|e| format!("Invalid GitHub release response: {e}"))
}

fn github_asset_response(
    source: &Source,
    release: &Release,
    asset: &Asset,
    stable_filename: String,
    require_checksum: bool,
) -> Result<GithubResponse, String> {
    if !safe_asset_url(&source.repository, &asset.browser_download_url) {
        return Err("Release asset URL is not within the configured GitHub repository".into());
    }
    let sha256 = match asset.digest.as_deref() {
        Some(value) => Some(sha256_digest(value).ok_or("Invalid GitHub SHA256 digest")?),
        None => {
            // The ordinary check also reads a published SHA256 sidecar. This
            // lets a manually downloaded stable-name AppImage be verified without
            // reinstalling it (notably shiftkey/desktop). Self-update retains
            // its strict missing/invalid-checksum behavior.
            let sidecar_name = format!("{}.sha256", asset.name);
            match release
                .assets
                .iter()
                .find(|entry| entry.name == sidecar_name)
            {
                Some(sidecar) => {
                    if !safe_asset_url(&source.repository, &sidecar.browser_download_url) {
                        return Err(
                            "Checksum URL is not within the configured GitHub repository".into(),
                        );
                    }
                    if sidecar.size == 0
                        || sidecar
                            .state
                            .as_deref()
                            .is_some_and(|state| state != "uploaded")
                    {
                        if require_checksum {
                            return Ok(GithubResponse::Pending {
                                tag: release.tag_name.clone(),
                                reason: "The SHA256 checksum upload is incomplete".into(),
                            });
                        }
                        None
                    } else {
                        match download_sha256_sidecar(&sidecar.browser_download_url, &asset.name) {
                            Ok(digest) => Some(digest),
                            Err(reason) if require_checksum => {
                                return Ok(GithubResponse::Pending {
                                    tag: release.tag_name.clone(),
                                    reason,
                                })
                            }
                            Err(_) => None, // Optional sidecar failure is not an update.
                        }
                    }
                }
                None if require_checksum => {
                    return Ok(GithubResponse::Pending {
                        tag: release.tag_name.clone(),
                        reason: "SHA256 sidecar is missing".into(),
                    })
                }
                None => None,
            }
        }
    };
    Ok(GithubResponse::Ready(ReleaseInfo {
        tag: release.tag_name.clone(),
        asset_name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        sha256,
        size: asset.size,
        stable_filename,
    }))
}

fn check_github(
    source: &Source,
    stable_filename: String,
    require_checksum: bool,
) -> Result<GithubResponse, String> {
    let api = format!(
        "https://api.github.com/repos/{}/releases/latest",
        source.repository
    );
    let release: Release = fetch_github_json(&api, "2097152")?;
    let absence_reason = match select_asset(&release, &source.asset_pattern, require_checksum) {
        Ok(AssetPick::Ready(asset)) => {
            return github_asset_response(
                source,
                &release,
                asset,
                stable_filename,
                require_checksum,
            )
        }
        // Never offer an older version when the newest matching AppImage is
        // still uploading. This also protects against incomplete checksum uploads.
        Ok(AssetPick::Pending(reason)) => {
            return Ok(GithubResponse::Pending {
                tag: release.tag_name.clone(),
                reason: reason.into(),
            })
        }
        Ok(AssetPick::NoAppImage) => "Latest release has no AppImage asset".to_owned(),
        Err(AssetSelectionError::WrongArchitecture(_)) if require_checksum => {
            return Ok(GithubResponse::Pending {
                tag: release.tag_name.clone(),
                reason: "The x86_64 AppImage has not been uploaded yet".into(),
            });
        }
        Err(AssetSelectionError::WrongArchitecture(names)) => {
            format!("Latest release has AppImages for other architectures only: {names}")
        }
        Err(reason) => return Err(reason.to_string()),
    };

    // Fallback is permitted only when the latest stable release lacks an
    // x86_64 AppImage. Pattern mismatch, ambiguity and incomplete uploads do
    // NOT silently downgrade to older releases. Self-update never falls back.
    if require_checksum {
        return Ok(GithubResponse::Pending {
            tag: release.tag_name.clone(),
            reason: absence_reason,
        });
    }
    let api = format!(
        "https://api.github.com/repos/{}/releases?per_page=12",
        source.repository
    );
    let releases: Vec<Release> = fetch_github_json(&api, "6291456")
        .map_err(|e| format!("Could not inspect older GitHub releases: {e}"))?;
    if let Some((older, asset)) =
        find_compatible_release(&releases, &release.tag_name, &source.asset_pattern)
    {
        return github_asset_response(source, older, asset, stable_filename, false);
    }
    Err(format!(
        "No compatible x86_64 AppImage in recent stable releases (latest: {}). {absence_reason}. Checked up to 12 releases.",
        release.tag_name))
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

/// Validate the AppImage runtime header, not only its filename. e_machine=62
/// means AMD64; this rejects an ARM64 executable even if its name is generic.
fn valid_x86_64_elf_header(header: &[u8]) -> bool {
    header.len() >= 20
        && &header[..4] == b"\x7fELF"
        && header[4] == 2
        && header[5] == 1
        && u16::from_le_bytes([header[18], header[19]]) == 62
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
        let mut elf_header = [0u8; 20];
        file.read_exact(&mut elf_header)
            .map_err(|e| format!("Invalid AppImage header: {e}"))?;
        if !valid_x86_64_elf_header(&elf_header) {
            return Err(
                "Downloaded AppImage is not an x86_64 ELF executable; update cancelled".into(),
            );
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
    fn github_http_status_is_parsed_without_corrupting_json() {
        let raw = b"{\"tag_name\":\"v1\",\"assets\":[]}\n__ZSOFT_GITHUB_HTTP__=200\n";
        let (status, body) = split_github_http_response(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, br#"{"tag_name":"v1","assets":[]}"#);
        assert!(split_github_http_response(b"no status").is_err());
    }

    #[test]
    fn github_http_errors_distinguish_rate_limit_permissions_and_missing_repo() {
        let limit = github_http_error(403, br#"{"message":"API rate limit exceeded for test"}"#);
        assert!(limit.starts_with("GitHub API rate limit exceeded"));
        assert!(github_http_error(429, b"{}").starts_with("GitHub API rate limit exceeded"));
        assert!(github_http_error(404, br#"{"message":"Not Found"}"#).contains("not found"));
        assert!(
            github_http_error(403, br#"{"message":"Resource not accessible"}"#)
                .contains("access denied")
        );
        assert!(github_http_error(502, b"{}").contains("temporarily unavailable"));
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
    fn manually_installed_versionless_appimage_with_matching_sha256_is_up_to_date() {
        let dir = fixture();
        let local = dir.join("Obsidian.AppImage");
        fs::write(&local, b"abc").unwrap();
        let info = ReleaseInfo {
            tag: "v1.14.4".into(),
            asset_name: "Obsidian-1.14.4.AppImage".into(),
            url: "https://github.com/obsidianmd/obsidian-releases/releases/download/v1.14.4/a"
                .into(),
            sha256: Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()),
            size: 3,
            stable_filename: "Obsidian.AppImage".into(),
        };
        assert!(local_matches_release_checksum(&local, &info));
        assert_eq!(
            installed_check_state(&local, None, &info),
            CheckState::UpToDate
        );
        assert_eq!(
            installed_check_state(&local, Some("v1.13.0"), &info),
            CheckState::UpToDate
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn manual_confirmation_requires_unchanged_file_and_matching_repository() {
        let dir = fixture();
        let path = dir.join("GitHubDesktop.AppImage");
        fs::write(&path, b"abc").unwrap();
        let record = config::LocalVersionConfirmation {
            repository: "shiftkey/desktop".into(),
            tag: "release-3.4.13-linux1".into(),
            sha256: local_sha256(&path).unwrap(),
            file_size: 3,
        };
        let info = ReleaseInfo {
            tag: record.tag.clone(),
            asset_name: "GitHubDesktop-linux-x86_64-3.4.13-linux1.AppImage".into(),
            url: String::new(),
            sha256: None,
            size: 3,
            stable_filename: "GitHubDesktop.AppImage".into(),
        };
        assert_eq!(
            checked_state_with_confirmation(&path, None, &info, "shiftkey/desktop", Some(&record)),
            CheckState::UserConfirmed
        );
        assert_eq!(
            checked_state_with_confirmation(&path, None, &info, "desktop/desktop", Some(&record)),
            CheckState::Uncertain
        );
        fs::write(&path, b"abd").unwrap();
        assert_eq!(
            checked_state_with_confirmation(&path, None, &info, "shiftkey/desktop", Some(&record)),
            CheckState::Uncertain
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn user_confirmed_older_version_allows_an_actual_update() {
        let dir = fixture();
        let path = dir.join("OpenPLC.Editor.AppImage");
        fs::write(&path, b"abc").unwrap();
        let record = config::LocalVersionConfirmation {
            repository: "Autonomy-Logic/openplc-editor".into(),
            tag: "v4.3.1".into(),
            sha256: local_sha256(&path).unwrap(),
            file_size: 3,
        };
        let info = ReleaseInfo {
            tag: "v4.3.2".into(),
            asset_name: "OpenPLC.Editor-4.3.2.AppImage".into(),
            url: String::new(),
            sha256: Some("0".repeat(64)),
            size: 3,
            stable_filename: "OpenPLC.Editor.AppImage".into(),
        };
        // The publisher hash does not match these sample bytes, but an
        // explicit user-confirmed *older* version still enables an update.
        assert_eq!(
            checked_state_with_confirmation(
                &path,
                None,
                &info,
                "Autonomy-Logic/openplc-editor",
                Some(&record)
            ),
            CheckState::Available
        );
        let current = config::LocalVersionConfirmation {
            tag: "v4.3.2".into(),
            ..record
        };
        assert_eq!(
            checked_state_with_confirmation(
                &path,
                None,
                &info,
                "Autonomy-Logic/openplc-editor",
                Some(&current)
            ),
            CheckState::UserConfirmed
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sidecar_digest_works_for_github_desktop_style() {
        let sidecar = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
        assert_eq!(
            parse_sha256_sidecar(sidecar, "GitHubDesktop-linux-x86_64-3.4.13-linux1.AppImage")
                .unwrap()
                .len(),
            64
        );
    }

    #[test]
    fn same_size_different_bytes_do_not_claim_latest_and_do_not_force_update() {
        let dir = fixture();
        let local = dir.join("Omniget.AppImage");
        fs::write(&local, b"xyz").unwrap();
        let info = ReleaseInfo {
            tag: "v0.12.0".into(),
            asset_name: "OmniGet_0.12.0_amd64.AppImage".into(),
            url: String::new(),
            sha256: Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()),
            size: 3,
            stable_filename: "Omniget.AppImage".into(),
        };
        assert!(!local_matches_release_checksum(&local, &info));
        assert_eq!(
            installed_check_state(&local, None, &info),
            CheckState::Uncertain
        );
        fs::write(&local, b"abcd").unwrap();
        assert!(!local_matches_release_checksum(&local, &info));
        assert_eq!(
            installed_check_state(&local, None, &info),
            CheckState::Uncertain
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pdfcraft_release_tag_differs_from_package_version_but_checksum_wins() {
        let dir = fixture();
        let local = dir.join("PDFCraft_0.1.0_amd64.AppImage");
        fs::write(&local, b"abc").unwrap();
        let info = ReleaseInfo {
            tag: "v2026.10.20-0503080".into(),
            asset_name: "PDFCraft_0.1.0_amd64.AppImage".into(),
            url: String::new(),
            sha256: Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()),
            size: 3,
            stable_filename: "PDFCraft.AppImage".into(),
        };
        assert_eq!(
            installed_check_state(&local, None, &info),
            CheckState::UpToDate
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_version_without_checksum_remains_unverified() {
        let dir = fixture();
        let local = dir.join("OpenPLC.Editor.AppImage");
        fs::write(&local, b"abc").unwrap();
        let info = ReleaseInfo {
            tag: "v4.3.2".into(),
            asset_name: "OpenPLC.Editor-4.3.2.AppImage".into(),
            url: String::new(),
            sha256: None,
            size: 3,
            stable_filename: "OpenPLC.Editor.AppImage".into(),
        };
        assert_eq!(
            installed_check_state(&local, None, &info),
            CheckState::Uncertain
        );
        assert_eq!(
            installed_check_state(&local, Some("v4.3.2"), &info),
            CheckState::UpToDate
        );
        fs::remove_dir_all(dir).unwrap();
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
    #[test]
    fn published_release_without_assets_is_pending_not_bad_pattern() {
        let release: Release =
            serde_json::from_str(r#"{"tag_name":"v0.4.3","assets":[]}"#).unwrap();
        assert!(matches!(
            select_asset(&release, "zSoft-Manager-*-x86_64.AppImage", true),
            Ok(AssetPick::Pending(_))
        ));
    }
    #[test]
    fn appimage_without_checksum_is_not_ready_for_self_update() {
        let release: Release = serde_json::from_str(r#"{"tag_name":"v0.4.3","assets":[
            {"name":"zSoft-Manager-v0.4.3-x86_64.AppImage","browser_download_url":"https://github.com/azislamayuda/zSoft-Manager/releases/download/v0.4.3/app","size":1024,"state":"uploaded"}
        ]}"#).unwrap();
        assert!(matches!(
            select_asset(&release, "zSoft-Manager-*-x86_64.AppImage", true),
            Ok(AssetPick::Pending(_))
        ));
        assert!(matches!(
            select_asset(&release, "zSoft-Manager-*-x86_64.AppImage", false),
            Ok(AssetPick::Ready(_))
        ));
    }
    #[test]
    fn other_appimage_name_is_configuration_error() {
        let release: Release = serde_json::from_str(r#"{"tag_name":"v1","assets":[
            {"name":"other-x86_64.AppImage","browser_download_url":"https://github.com/owner/repo/releases/download/v1/other","size":1}
        ]}"#).unwrap();
        assert!(select_asset(&release, "zSoft-Manager-*.AppImage", true).is_err());
    }
    #[test]
    fn checksum_sidecar_parser_rejects_another_filename() {
        let hash = "a".repeat(64);
        assert!(
            parse_sha256_sidecar(&format!("{hash}  other.AppImage"), "zSoft.AppImage").is_none()
        );
        assert_eq!(
            parse_sha256_sidecar(&format!("{hash} *zSoft.AppImage"), "zSoft.AppImage"),
            Some(hash)
        );
    }
    #[test]
    fn valid_digest_can_make_release_ready_without_sidecar() {
        let release: Release = serde_json::from_str(&format!(r#"{{"tag_name":"v0.4.3","assets":[
            {{"name":"zSoft-Manager-v0.4.3-x86_64.AppImage","browser_download_url":"https://github.com/azislamayuda/zSoft-Manager/releases/download/v0.4.3/app","size":1024,"state":"uploaded","digest":"sha256:{}"}}
        ]}}"#, "a".repeat(64))).unwrap();
        assert!(matches!(
            select_asset(&release, "zSoft-Manager-*-x86_64.AppImage", true),
            Ok(AssetPick::Ready(_))
        ));
    }

    #[test]
    fn default_pattern_accepts_x64_amd64_and_unsuffixed_appimages() {
        for filename in [
            "GitHub-Copilot-linux-x64.AppImage",
            "PDFCraft_1.2.0_amd64.AppImage",
            "Obsidian-1.13.7.AppImage",
        ] {
            let release: Release = serde_json::from_str(&format!(r#"{{"tag_name":"v1.2.0","assets":[
                {{"name":"{}","browser_download_url":"https://github.com/x/y/releases/download/v1/a","size":100}}
            ]}}"#, filename)).unwrap();
            match select_asset(&release, "*x86_64*.AppImage", false) {
                Ok(AssetPick::Ready(asset)) => assert_eq!(asset.name, filename),
                _ => panic!("expected compatible x86_64 AppImage: {filename}"),
            }
        }
    }
    #[test]
    fn arm_asset_is_never_selected_by_generic_pattern() {
        let release: Release = serde_json::from_str(r#"{"tag_name":"v1","assets":[
            {"name":"Trilium-linux-arm64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/arm","size":1},
            {"name":"Trilium-linux-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/x64","size":1}
        ]}"#).unwrap();
        match select_asset(&release, "*x86_64*.AppImage", false) {
            Ok(AssetPick::Ready(asset)) => assert_eq!(asset.name, "Trilium-linux-x64.AppImage"),
            _ => panic!("expected only x64 selection"),
        }
    }
    #[test]
    fn custom_pattern_is_never_overridden() {
        let release: Release = serde_json::from_str(r#"{"tag_name":"v1","assets":[
            {"name":"Trilium-linux-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/x64","size":1}
        ]}"#).unwrap();
        assert!(select_asset(&release, "OtherProduct-*.AppImage", false).is_err());
    }
    #[test]
    fn mobile_only_latest_falls_back_to_recent_stable_desktop_release() {
        let releases: Vec<Release> = serde_json::from_str(r#"[
            {"tag_name":"1.13.8","assets":[]},
            {"tag_name":"1.13.7","assets":[]},
            {"tag_name":"1.13.6","assets":[
                {"name":"Obsidian-1.13.6.AppImage","browser_download_url":"https://github.com/obsidianmd/obsidian-releases/releases/download/1.13.6/Obsidian-1.13.6.AppImage","size":123}
            ]}
        ]"#).unwrap();
        let (release, asset) =
            find_compatible_release(&releases, "1.13.8", "*x86_64*.AppImage").unwrap();
        assert_eq!(release.tag_name, "1.13.6");
        assert_eq!(asset.name, "Obsidian-1.13.6.AppImage");
    }
    #[test]
    fn seven_publisher_asset_name_regressions() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/github-appimage-regressions.json"
        ))
        .unwrap();
        let cases = fixtures["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 7);
        for case in cases {
            let filenames = case["assets"].as_array().unwrap();
            let assets: Vec<serde_json::Value> = filenames.iter().map(|filename| {
                serde_json::json!({
                    "name": filename.as_str().unwrap(),
                    "browser_download_url": format!("https://github.com/{}/releases/download/test/asset",
                        case["repository"].as_str().unwrap()),
                    "size": 4096,
                    "state": "uploaded"
                })
            }).collect();
            let release: Release = serde_json::from_value(serde_json::json!({
                "tag_name": "test", "assets": assets
            }))
            .unwrap();
            match select_asset(&release, case["pattern"].as_str().unwrap(), false) {
                Ok(AssetPick::Ready(asset)) => assert_eq!(
                    asset.name,
                    case["expected"].as_str().unwrap(),
                    "{}",
                    case["application"]
                ),
                _ => panic!("Could not select {}", case["application"]),
            }
        }
    }

    #[test]
    fn empty_release_and_foreign_architecture_are_distinct() {
        let no_assets: Release = serde_json::from_str(
            r#"{"tag_name":"latest","assets":[{"name":"Tool.deb","browser_download_url":"https://github.com/x/y/releases/download/v1/a","size":100}]}"#
        ).unwrap();
        assert!(matches!(
            select_asset(&no_assets, "*.AppImage", false),
            Ok(AssetPick::NoAppImage)
        ));
        let arm_only: Release = serde_json::from_str(
            r#"{"tag_name":"latest","assets":[{"name":"Tool-arm64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/a","size":100}]}"#
        ).unwrap();
        assert!(matches!(
            select_asset(&arm_only, "*.AppImage", false),
            Err(AssetSelectionError::WrongArchitecture(_))
        ));
    }

    #[test]
    fn pattern_mismatch_and_multiple_candidates_are_distinct() {
        let release: Release = serde_json::from_str(r#"{"tag_name":"v1","assets":[
            {"name":"Alpha-linux-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/a","size":100},
            {"name":"Beta-linux-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/b","size":100}
        ]}"#).unwrap();
        assert!(matches!(
            select_asset(&release, "Gamma-*.AppImage", false),
            Err(AssetSelectionError::PatternMismatch { .. })
        ));
        assert!(matches!(
            select_asset(&release, "*x86_64*.AppImage", false),
            Err(AssetSelectionError::Ambiguous { .. })
        ));
    }

    #[test]
    fn incomplete_latest_release_must_not_be_selected_by_fallback() {
        let latest: Release = serde_json::from_str(r#"{"tag_name":"v3","assets":[
            {"name":"App-linux-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v3/a","size":0,"state":"starter"}
        ]}"#).unwrap();
        assert!(matches!(
            select_asset(&latest, "*x86_64*.AppImage", false),
            Ok(AssetPick::Pending(_))
        ));
        let older: Vec<Release> = serde_json::from_str(r#"[
            {"tag_name":"v2","prerelease":true,"assets":[
                {"name":"App-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v2/a","size":100}
            ]},
            {"tag_name":"v1","assets":[
                {"name":"App-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/a","size":100}
            ]}
        ]"#).unwrap();
        let (found, _) = find_compatible_release(&older, "v3", "*x86_64*.AppImage").unwrap();
        assert_eq!(found.tag_name, "v1");
        // check_github handles Pending before it ever calls find_compatible_release.
    }

    #[test]
    fn downloaded_elf_header_rejects_arm64_and_32_bit() {
        let mut header = [0u8; 20];
        header[..4].copy_from_slice(b"\x7fELF");
        header[4] = 2; // 64-bit ELF
        header[5] = 1; // little-endian
        header[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert!(valid_x86_64_elf_header(&header));
        header[18..20].copy_from_slice(&183u16.to_le_bytes()); // ARM64
        assert!(!valid_x86_64_elf_header(&header));
        header[18..20].copy_from_slice(&62u16.to_le_bytes());
        header[4] = 1; // 32-bit
        assert!(!valid_x86_64_elf_header(&header));
        assert!(!valid_x86_64_elf_header(b"not an AppImage"));
    }

    #[test]
    fn newer_only_mobile_release_uses_latest_compatible_stable_fallback() {
        let releases: Vec<Release> = serde_json::from_str(r#"[
            {"tag_name":"1.13.8","assets":[
                {"name":"Obsidian-1.13.8.apk","browser_download_url":"https://github.com/x/y/releases/download/1.13.8/a","size":100}
            ]},
            {"tag_name":"1.13.7","assets":[
                {"name":"Obsidian-1.13.7-arm64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/1.13.7/b","size":200},
                {"name":"Obsidian-1.13.7.AppImage","browser_download_url":"https://github.com/x/y/releases/download/1.13.7/c","size":200}
            ]}
        ]"#).unwrap();
        let (found, asset) =
            find_compatible_release(&releases, "1.13.8", "*x86_64*.AppImage").unwrap();
        assert_eq!(found.tag_name, "1.13.7");
        assert_eq!(asset.name, "Obsidian-1.13.7.AppImage");
    }

    #[test]
    fn no_compatible_asset_does_not_select_arm_or_prerelease() {
        let releases: Vec<Release> = serde_json::from_str(r#"[
            {"tag_name":"v2","prerelease":true,"assets":[
                {"name":"Tool-x64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v2/x64","size":1}
            ]},
            {"tag_name":"v1","assets":[
                {"name":"Tool-arm64.AppImage","browser_download_url":"https://github.com/x/y/releases/download/v1/arm","size":1}
            ]}
        ]"#).unwrap();
        assert!(find_compatible_release(&releases, "v3", "*x86_64*.AppImage").is_none());
    }
}
