//! NVM / Node.js / npm integration. No permanent process or startup network requests.
//! All NVM operations run in an isolated, short-lived Bash subprocess. They do not
//! change the environment of existing terminal windows.
use serde::Deserialize;
use std::{cmp::Ordering, env, fs, path::PathBuf, process::Command};

#[derive(Clone, Debug, Default)]
pub struct Versions {
    pub nvm: Option<String>,
    pub nvm_latest: Option<String>,
    pub node_active: Option<String>,
    pub node_path: Option<String>,
    pub default_node: Option<String>,
    pub default_alias: Option<String>,
    pub lts: Option<String>,
    pub lts_latest: Option<String>,
    pub npm: Option<String>,
    pub npm_latest: Option<String>,
    pub current: Option<String>,
    pub current_latest: Option<String>,
    pub installed_nodes: Vec<InstalledNode>,
    pub global_packages: Vec<GlobalPackage>,
    pub global_updates_checked: bool,
    pub nvm_available: bool,
    pub nvm_git_checkout: bool,
    pub warnings: Vec<String>,
    pub schedule: ReleaseSchedule,
    pub checked_on: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledNode {
    pub version: String,
    pub phase: NodePhase,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalPackage {
    pub name: String,
    pub installed: String,
    pub latest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodePhase {
    ActiveLts(String),
    MaintenanceLts(String),
    Current,
    Eol,
    Unknown,
}
impl NodePhase {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ActiveLts(_) | Self::MaintenanceLts(_) => "LTS",
            Self::Current => "Current",
            Self::Eol => "EOL",
            Self::Unknown => "Unknown",
        }
    }
    pub fn detail(&self) -> String {
        match self {
            Self::ActiveLts(c) => {
                if c.is_empty() {
                    "Active LTS".into()
                } else {
                    format!("Active LTS · {c}")
                }
            }
            Self::MaintenanceLts(c) => {
                if c.is_empty() {
                    "Maintenance LTS".into()
                } else {
                    format!("Maintenance LTS · {c}")
                }
            }
            Self::Current => "Current release".into(),
            Self::Eol => "End of life (unsupported)".into(),
            Self::Unknown => "Lifecycle unknown".into(),
        }
    }
    pub fn css(&self) -> &'static str {
        match self {
            Self::ActiveLts(_) | Self::MaintenanceLts(_) => "node-badge-lts",
            Self::Current => "node-badge-current",
            Self::Eol => "node-badge-eol",
            Self::Unknown => "node-badge-unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    Nvm,
    Lts,
    Npm,
    Current,
}

pub fn nvm_dir() -> PathBuf {
    env::var_os("NVM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".nvm")
        })
}

// Official Node.js Release Working Group's schedule, bundled for offline usage.
// It may be refreshed only by explicitly clicking Check updates.
const BUNDLED_SCHEDULE: &str = include_str!("../assets/node-release-schedule.json");
const SCHEDULE_URL: &str = "https://raw.githubusercontent.com/nodejs/Release/main/schedule.json";

#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseCycle {
    pub start: Option<String>,
    pub lts: Option<String>,
    pub maintenance: Option<String>,
    pub end: String,
    #[serde(default)]
    pub codename: String,
}

#[derive(Clone, Debug, Default)]
pub struct ReleaseSchedule {
    pub cycles: std::collections::HashMap<String, ReleaseCycle>,
}
impl ReleaseSchedule {
    pub fn parse(json: &str) -> Result<Self, String> {
        let cycles = serde_json::from_str(json)
            .map_err(|e| format!("Invalid Node release schedule: {e}"))?;
        Ok(Self { cycles })
    }
    pub fn classify(&self, version: &str, date: &str) -> NodePhase {
        let Some(major) = numbers(version).and_then(|v| v.first().copied()) else {
            return NodePhase::Unknown;
        };
        let Some(cycle) = self.cycles.get(&format!("v{major}")) else {
            return NodePhase::Unknown;
        };
        if !is_date(date) {
            return NodePhase::Unknown;
        }
        if date >= cycle.end.as_str() {
            return NodePhase::Eol;
        }
        if cycle.start.as_deref().is_none_or(|start| date < start) {
            return NodePhase::Unknown;
        }
        if let Some(lts_start) = cycle.lts.as_deref() {
            if date >= lts_start {
                if cycle.maintenance.as_deref().is_some_and(|m| date >= m) {
                    return NodePhase::MaintenanceLts(cycle.codename.clone());
                }
                return NodePhase::ActiveLts(cycle.codename.clone());
            }
        }
        NodePhase::Current
    }
}
fn is_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
}

const LOCAL: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
printf 'today=%s\n' "$(date -u +%F)"
if [ -s "$NVM_DIR/nvm.sh" ]; then
    . "$NVM_DIR/nvm.sh" --no-use
    printf 'nvm=%s\n' "$(nvm --version)"
    printf 'lts=%s\n' "$(nvm version 'lts/*' 2>/dev/null || true)"
    printf 'default=%s\n' "$(nvm version default 2>/dev/null || true)"
    # In-app active version: a fresh NVM shell using its configured default.
    # It cannot change an already-running terminal's active Node selection.
    nvm use default >/dev/null 2>&1 || true
fi
if command -v node >/dev/null 2>&1; then
    printf 'node=%s\n' "$(node -v)"
    printf 'path=%s\n' "$(command -v node)"
fi
if command -v npm >/dev/null 2>&1; then
    printf 'npm=%s\n' "$(npm -v)"
fi
"#;

fn bash(script: &str, target: Option<&str>) -> Result<String, String> {
    let mut command = Command::new("bash");
    command.arg("-c").arg(script).arg("zsoft-node");
    if let Some(target) = target {
        command.arg(target);
    }
    command.env("NVM_DIR", nvm_dir());
    let output = command
        .output()
        .map_err(|e| format!("Unable to start Bash: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else if !stdout.trim().is_empty() {
            stdout.trim()
        } else {
            "NVM returned a failure without diagnostic output"
        };
        let detail = detail.chars().take(1500).collect::<String>();
        return Err(format!("Command failed ({}): {detail}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
fn optional(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || matches!(value, "N/A" | "none" | "system" | "Not installed") {
        None
    } else {
        Some(value.to_owned())
    }
}
fn parse_local(out: &str) -> Versions {
    let mut v = Versions::default();
    for line in out.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "nvm" => {
                v.nvm = optional(value);
                v.nvm_available = v.nvm.is_some();
            }
            "lts" => v.lts = optional(value),
            "node" => v.node_active = optional(value),
            "path" => v.node_path = optional(value),
            "npm" => v.npm = optional(value),
            "default" => v.default_node = optional(value),
            "today" => v.checked_on = value.trim().to_owned(),
            _ => {}
        }
    }
    v
}

pub fn numbers(value: &str) -> Option<Vec<u64>> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    let nums: Vec<_> = value
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<_, _>>()
        .ok()?;
    (nums.len() == 3).then_some(nums)
}
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    Some(numbers(a)?.cmp(&numbers(b)?))
}
pub fn newer(installed: Option<&str>, latest: Option<&str>) -> bool {
    let Some(latest) = latest else { return false };
    match installed {
        None => numbers(latest).is_some(),
        Some(old) => compare(old, latest) == Some(Ordering::Less),
    }
}
pub fn latest_nvm_tag(listing: &str) -> Option<String> {
    listing
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
        .filter_map(|name| name.rsplit('/').next())
        .filter(|tag| tag.starts_with('v') && numbers(tag).is_some())
        .max_by(|a, b| compare(a, b).unwrap_or(Ordering::Equal))
        .map(ToOwned::to_owned)
}
pub fn current_installed(versions: &[String], latest_current: &str) -> Option<String> {
    let major = *numbers(latest_current)?.first()?;
    versions
        .iter()
        .filter(|version| numbers(version).and_then(|v| v.first().copied()) == Some(major))
        .max_by(|a, b| compare(a, b).unwrap_or(Ordering::Equal))
        .cloned()
}

pub fn installed_node_versions(
    dir: &std::path::Path,
    schedule: &ReleaseSchedule,
    date: &str,
) -> Vec<InstalledNode> {
    let mut versions: Vec<_> = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|ty| ty.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|version| numbers(version).is_some())
        .collect();
    versions.sort_by(|a, b| compare(b, a).unwrap_or(Ordering::Equal));
    versions
        .into_iter()
        .map(|version| InstalledNode {
            phase: schedule.classify(&version, date),
            version,
        })
        .collect()
}

/// Suggest cleanup only when a newer patch/minor of the same Node major is installed.
/// Never suggest removing the Node selected by the current NVM default alias or
/// the Node active in the application's isolated NVM shell.
pub fn old_node_cleanup_candidate(
    installed: &[InstalledNode],
    target: &str,
    default: Option<&str>,
    active: Option<&str>,
) -> bool {
    let Some(target_parts) = numbers(target) else {
        return false;
    };
    if !target.starts_with('v') || Some(target) == default || Some(target) == active {
        return false;
    }
    installed.iter().any(|other| {
        numbers(&other.version)
            .is_some_and(|candidate| candidate[0] == target_parts[0] && candidate > target_parts)
    })
}

/// Only uninstall a superseded NVM-managed version. This deliberately does not
/// remove a whole major line, EOL versions, or system Node installations.
/// Existing terminals and global npm packages in the removed version may be affected.
pub fn remove_old_node_version(version: &str) -> Result<String, String> {
    if !version.starts_with('v') || numbers(version).is_none() {
        return Err("Invalid NVM Node.js version; removal cancelled".into());
    }
    let dir = nvm_dir();
    if !dir.join("nvm.sh").is_file() {
        return Err("NVM is unavailable; removal cancelled".into());
    }
    let installed =
        installed_node_versions(&dir.join("versions/node"), &ReleaseSchedule::default(), "");
    if !old_node_cleanup_candidate(&installed, version, None, None) {
        return Err(
            "No newer installed Node.js release of the same major version; removal cancelled"
                .into(),
        );
    }
    // A second, live safety check occurs inside the NVM subprocess just before
    // deleting anything, including resolving the current default alias.
    const REMOVE_OLD: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
target="$1"
[[ "$target" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Invalid Node.js version' >&2; exit 1; }
[ -f "$NVM_DIR/versions/node/$target/bin/node" ] || { echo 'Target is not an installed NVM Node.js version' >&2; exit 1; }
resolved_default="$(nvm version default 2>/dev/null || true)"
if [ "$resolved_default" = "$target" ]; then
    echo 'Refusing to remove the NVM default version' >&2
    exit 1
fi
# This app activates NVM default in its isolated inspection shell.
# Ignore the inherited terminal PATH here: it can still point at the old
# version even when zSoft Manager uses the newer NVM default. Existing open
# terminals are warned in the UI before uninstall.
# Recheck there is still a newer version in this major line immediately before deletion.
major="${target#v}"
major="${major%%.*}"
newer_found=0
for candidate_dir in "$NVM_DIR"/versions/node/v"$major".*; do
    [ -f "$candidate_dir/bin/node" ] || continue
    candidate="${candidate_dir##*/}"
    [[ "$candidate" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
    if [ "$candidate" != "$target" ] && [ "$(printf '%s\n' "$target" "$candidate" | sort -V | tail -n 1)" = "$candidate" ]; then
        newer_found=1
        break
    fi
done
[ "$newer_found" -eq 1 ] || { echo 'No newer same-major Node.js remains installed' >&2; exit 1; }
# All changes are isolated to this short-lived Bash process.
# NVM cannot uninstall a version currently selected by the subprocess.
nvm use default >/dev/null 2>&1 || { echo 'Could not safely select NVM default' >&2; exit 1; }
# Check the Node actually active in the subprocess that performs uninstall.
[ "$(nvm current 2>/dev/null || true)" != "$target" ] || { echo 'Target still active after selecting default; removal refused' >&2; exit 1; }
nvm uninstall "$target"
"#;
    bash(REMOVE_OLD, Some(version))?;
    Ok(format!("Removed old Node.js {version} using NVM. Its version-specific global npm packages were also removed."))
}

pub fn valid_global_package(name: &str) -> bool {
    if name.len() > 214 || name.is_empty() || name.starts_with('-') {
        return false;
    }
    let simple = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~'))
    };
    if let Some(scoped) = name.strip_prefix('@') {
        let Some((scope, pkg)) = scoped.split_once('/') else {
            return false;
        };
        simple(scope) && simple(pkg) && !pkg.contains('/')
    } else {
        simple(name)
    }
}

#[derive(Deserialize)]
struct NpmInventory {
    #[serde(default)]
    dependencies: std::collections::HashMap<String, NpmInstalled>,
}
#[derive(Deserialize)]
struct NpmInstalled {
    version: Option<String>,
}
#[derive(Deserialize)]
struct NpmOutdated {
    current: Option<String>,
    latest: Option<String>,
}

fn parse_global_inventory(text: &str) -> Result<Vec<GlobalPackage>, String> {
    let inventory: NpmInventory =
        serde_json::from_str(text).map_err(|e| format!("Invalid npm inventory: {e}"))?;
    let mut list: Vec<_> = inventory
        .dependencies
        .into_iter()
        .filter(|(name, _)| valid_global_package(name))
        .map(|(name, pkg)| GlobalPackage {
            name,
            installed: pkg.version.unwrap_or_else(|| "Unknown".into()),
            latest: None,
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}
fn parse_global_outdated(text: &str, list: &mut [GlobalPackage]) -> Result<(), String> {
    let outdated: std::collections::HashMap<String, NpmOutdated> =
        serde_json::from_str(text).map_err(|e| format!("Invalid npm outdated response: {e}"))?;
    for pkg in list {
        if let Some(item) = outdated.get(&pkg.name) {
            pkg.latest = item
                .latest
                .clone()
                .filter(|latest| newer(Some(&pkg.installed), Some(latest)));
            if pkg.installed == "Unknown" {
                if let Some(current) = &item.current {
                    pkg.installed = current.clone();
                }
            }
        }
    }
    Ok(())
}

// Isolated npm commands always resolve the *default NVM-managed Node*; they
// never invoke system npm with sudo and never run just "npm update -g".
const GLOBAL_LIST: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
nvm use default >/dev/null 2>&1
npm list --global --depth=0 --json --silent
"#;
const GLOBAL_OUTDATED: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
nvm use default >/dev/null 2>&1
set +e
result="$(npm outdated --global --depth=0 --json --silent)"
status=$?
set -e
if [ "$status" -ne 0 ] && [ "$status" -ne 1 ]; then
    echo "npm outdated failed (exit $status)" >&2
    exit "$status"
fi
if [ -z "$result" ]; then
    printf '{}\n'
else
    printf '%s\n' "$result"
fi
"#;
const GLOBAL_UPDATE: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
nvm use default >/dev/null 2>&1
npm update --global -- "$1"
"#;

pub fn update_global_package(name: &str) -> Result<String, String> {
    if !valid_global_package(name) || name == "npm" {
        return Err("Invalid or reserved npm package name".into());
    }
    if !nvm_dir().join("nvm.sh").is_file() {
        return Err("NVM is unavailable".into());
    }
    bash(GLOBAL_UPDATE, Some(name))?;
    Ok(format!(
        "Global npm package {name} updated. Recheck packages to verify."
    ))
}

pub fn inspect(check_remote: bool) -> Result<Versions, String> {
    let mut v = parse_local(&bash(LOCAL, None)?);
    v.nvm_git_checkout = nvm_dir().join(".git").is_dir();
    v.default_alias = fs::read_to_string(nvm_dir().join("alias/default"))
        .ok()
        .and_then(|s| optional(&s));
    v.schedule = ReleaseSchedule::parse(BUNDLED_SCHEDULE)?;
    if check_remote {
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
                "8",
                "--max-time",
                "20",
                SCHEDULE_URL,
            ])
            .output();
        match out {
            Ok(out) if out.status.success() => {
                match ReleaseSchedule::parse(&String::from_utf8_lossy(&out.stdout)) {
                    Ok(schedule) => v.schedule = schedule,
                    Err(_) => v.warnings.push(
                        "Latest Node lifecycle schedule is invalid; using bundled schedule.".into(),
                    ),
                }
            }
            _ => v
                .warnings
                .push("Could not refresh Node lifecycle schedule; using bundled schedule.".into()),
        }
    }
    v.installed_nodes =
        installed_node_versions(&nvm_dir().join("versions/node"), &v.schedule, &v.checked_on);
    if v.nvm_available && v.default_node.is_some() {
        match bash(GLOBAL_LIST, None).and_then(|s| parse_global_inventory(&s)) {
            Ok(packages) => v.global_packages = packages,
            Err(e) => v
                .warnings
                .push(format!("Could not read global npm packages: {e}")),
        }
    }
    if !check_remote {
        return Ok(v);
    }
    if !v.nvm_available {
        v.warnings
            .push("NVM is not installed. System Node.js packages will not be modified.".into());
        return Ok(v);
    }
    let git = Command::new("git")
        .args([
            "ls-remote",
            "--tags",
            "--refs",
            "https://github.com/nvm-sh/nvm.git",
            "v*",
        ])
        .output();
    match git {
        Ok(out) if out.status.success() => {
            v.nvm_latest = latest_nvm_tag(&String::from_utf8_lossy(&out.stdout))
        }
        _ => v
            .warnings
            .push("Could not check the latest NVM tag.".into()),
    }
    const REMOTE: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
nvm use default >/dev/null 2>&1 || true
case "$1" in
    lts) nvm version-remote --lts ;;
    current) nvm version-remote node ;;
    npm) npm view npm version --fetch-retries=0 --fetch-timeout=15000 ;;
esac
"#;
    for (which, title) in [
        ("lts", "Node LTS"),
        ("current", "Node Current"),
        ("npm", "npm"),
    ] {
        match bash(REMOTE, Some(which)) {
            Ok(out) => {
                let latest = out.lines().last().and_then(optional);
                match which {
                    "lts" => v.lts_latest = latest,
                    "current" => v.current_latest = latest,
                    _ => v.npm_latest = latest,
                }
            }
            Err(_) => v.warnings.push(format!("Could not check {title}.")),
        }
    }
    if let Some(latest) = &v.current_latest {
        let versions: Vec<_> = v
            .installed_nodes
            .iter()
            .map(|entry| entry.version.clone())
            .collect();
        v.current = current_installed(&versions, latest);
    }
    if !v.global_packages.is_empty() {
        match bash(GLOBAL_OUTDATED, None)
            .and_then(|s| parse_global_outdated(&s, &mut v.global_packages))
        {
            Ok(()) => v.global_updates_checked = true,
            Err(e) => v
                .warnings
                .push(format!("Could not check global npm package updates: {e}")),
        }
    } else if v.nvm_available && v.default_node.is_some() {
        // An empty global npm inventory is still a successful state.
        v.global_updates_checked = true;
    }
    Ok(v)
}

pub fn set_default_version(version: &str) -> Result<String, String> {
    if numbers(version).is_none() {
        return Err("Invalid Node.js version".into());
    }
    let dir = nvm_dir();
    let installed = dir.join("versions/node").join(version);
    if !installed.join("bin/node").is_file() {
        return Err("The selected Node.js version is not installed in NVM".into());
    }
    const SET_DEFAULT: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
nvm alias default "$1" >/dev/null
"#;
    bash(SET_DEFAULT, Some(version))?;
    Ok(format!("Default Node.js set to {version}. New NVM shells will use it; existing terminals are unchanged."))
}

pub fn install(which: Install, newest: &str) -> Result<String, String> {
    if numbers(newest).is_none() {
        return Err("Invalid version string: installation cancelled".into());
    }
    if !nvm_dir().join("nvm.sh").is_file() {
        return Err("NVM is missing; installation cancelled".into());
    }
    const NVM: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
[ -d "$NVM_DIR/.git" ] || { echo 'NVM is not a Git checkout. Update NVM manually.' >&2; exit 1; }
git -C "$NVM_DIR" fetch --tags --quiet
git -C "$NVM_DIR" checkout --quiet "$1"
"#;
    const LTS: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
previous="$(nvm version 'lts/*' 2>/dev/null || true)"
if [[ "$previous" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    nvm install --lts --reinstall-packages-from="$previous"
else
    nvm install --lts
fi
nvm alias default 'lts/*'
"#;
    const NPM: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
nvm use --lts
nvm install-latest-npm
"#;
    const CURRENT: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
. "$NVM_DIR/nvm.sh" --no-use
old_default="$(cat "$NVM_DIR/alias/default" 2>/dev/null || true)"
previous="$(nvm version node 2>/dev/null || true)"
previous_major="${previous#v}"
previous_major="${previous_major%%.*}"
target_major="${1#v}"
target_major="${target_major%%.*}"
if [[ "$previous" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] && [ "$previous_major" = "$target_major" ] && [ "$previous" != "$1" ]; then
    # Keep global npm packages before optionally removing the superseded version.
    nvm install "$1" --reinstall-packages-from="$previous"
else
    nvm install "$1"
fi
if [ -n "$old_default" ]; then nvm alias default "$old_default"; fi
"#;
    let script = match which {
        Install::Nvm => NVM,
        Install::Lts => LTS,
        Install::Npm => NPM,
        Install::Current => CURRENT,
    };
    bash(script, Some(newest))?;
    Ok(match which {
        Install::Nvm => "NVM update finished".into(),
        Install::Lts => "Node.js LTS update finished; LTS remains the default".into(),
        Install::Npm => {
            "npm updated to the latest version compatible with your LTS installation".into()
        }
        Install::Current => {
            "Node.js Current installed; your previous default alias was preserved".into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_from_official_schedule() {
        let schedule = ReleaseSchedule::parse(BUNDLED_SCHEDULE).unwrap();
        assert_eq!(
            schedule.classify("v24.22.0", "2026-10-21"),
            NodePhase::MaintenanceLts("Krypton".into())
        );
        assert_eq!(
            schedule.classify("v26.11.0", "2026-10-21"),
            NodePhase::Current
        );
        assert_eq!(
            schedule.classify("v22.10.0", "2026-10-21"),
            NodePhase::MaintenanceLts("Jod".into())
        );
        assert_eq!(schedule.classify("v25.1.0", "2026-10-21"), NodePhase::Eol);
        assert_eq!(
            schedule.classify("v26.11.0", "2026-11-01"),
            NodePhase::ActiveLts("".into())
        );
        assert_eq!(
            schedule.classify("v99.0.0", "2026-10-21"),
            NodePhase::Unknown
        );
    }
    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare("v20.9.0", "v20.10.0"), Some(Ordering::Less));
        assert!(newer(Some("v1.2.3"), Some("v1.2.4")));
        assert!(!newer(Some("v1.2.4"), Some("v1.2.3")));
        assert!(!newer(Some("v1.2.3"), Some("bad")));
    }
    #[test]
    fn parses_installed_versions_and_default() {
        let v = parse_local("nvm=0.40.2\nlts=v24.3.0\nnode=v24.3.0\nnpm=11.4.2\npath=/home/u/.nvm/node\ndefault=v24.3.0\ntoday=2026-10-21\n");
        assert!(v.nvm_available);
        assert_eq!(v.default_node.as_deref(), Some("v24.3.0"));
        assert_eq!(v.checked_on, "2026-10-21");
    }
    #[test]
    fn latest_tags_and_current_selection() {
        let tags = "aaa\trefs/tags/v0.39.0\nbbb\trefs/tags/v0.40.1\nccc\trefs/tags/v0.40.3\n";
        assert_eq!(latest_nvm_tag(tags).as_deref(), Some("v0.40.3"));
        let versions = vec!["v24.9.0".into(), "v25.1.0".into(), "v25.2.0".into()];
        assert_eq!(
            current_installed(&versions, "v25.3.0").as_deref(),
            Some("v25.2.0")
        );
    }
    #[test]
    fn installed_version_sort_and_filter() {
        let root = std::env::temp_dir().join(format!("zsoft-node-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("v24.2.0")).unwrap();
        std::fs::create_dir_all(root.join("v22.14.0")).unwrap();
        std::fs::create_dir_all(root.join("not-version")).unwrap();
        let schedule = ReleaseSchedule::parse(BUNDLED_SCHEDULE).unwrap();
        let nodes = installed_node_versions(&root, &schedule, "2026-10-21");
        assert_eq!(
            nodes.iter().map(|n| n.version.as_str()).collect::<Vec<_>>(),
            vec!["v24.2.0", "v22.14.0"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn global_npm_parsing_and_selection() {
        let mut list = parse_global_inventory(r#"{"dependencies":{"typescript":{"version":"5.9.0"},"npm":{"version":"11.4.0"},"@scope/package":{"version":"1.0.0"}}}"#).unwrap();
        parse_global_outdated(
            r#"{"typescript":{"current":"5.9.0","latest":"6.0.0"}}"#,
            &mut list,
        )
        .unwrap();
        assert_eq!(
            list.iter()
                .find(|p| p.name == "typescript")
                .unwrap()
                .latest
                .as_deref(),
            Some("6.0.0")
        );
        assert!(list
            .iter()
            .find(|p| p.name == "@scope/package")
            .unwrap()
            .latest
            .is_none());
        assert!(valid_global_package("@scope/tool"));
        assert!(!valid_global_package("--global"));
        assert!(!valid_global_package("a/b/c"));
    }

    #[test]
    fn node_cleanup_only_offers_older_same_major_versions() {
        let entries = ["v26.12.1", "v26.12.0", "v24.22.1", "v24.22.0", "v22.14.0"]
            .into_iter()
            .map(|version| InstalledNode {
                version: version.into(),
                phase: NodePhase::Unknown,
            })
            .collect::<Vec<_>>();
        let candidate =
            |name, default, active| old_node_cleanup_candidate(&entries, name, default, active);
        assert!(candidate("v26.12.0", Some("v24.22.1"), Some("v24.22.1")));
        assert!(candidate("v24.22.0", Some("v24.22.1"), Some("v24.22.1")));
        assert!(!candidate("v26.12.1", Some("v24.22.1"), Some("v24.22.1")));
        assert!(!candidate("v24.22.1", Some("v24.22.1"), Some("v24.22.1")));
        assert!(!candidate("v22.14.0", Some("v24.22.1"), Some("v24.22.1")));
        assert!(!candidate("v26.12.0", Some("v26.12.0"), None));
        assert!(!candidate("v24.22.0", None, Some("v24.22.0")));
        assert!(!candidate("../v26.12.0", None, None));
    }

    #[test]
    fn node_cleanup_never_targets_the_older_major_just_because_it_is_eol() {
        let entries = ["v26.12.1", "v24.22.1", "v22.14.0"]
            .into_iter()
            .map(|version| InstalledNode {
                version: version.into(),
                phase: NodePhase::Eol,
            })
            .collect::<Vec<_>>();
        assert!(!old_node_cleanup_candidate(
            &entries, "v22.14.0", None, None
        ));
    }

    #[test]
    fn npm_outdated_shell_output_is_valid_json() {
        // Regression: Bash expansion "${result:-{}}" appended an extra '}' to
        // both empty and non-empty results, breaking serde_json parsing.
        let temp = std::env::temp_dir().join(format!(
            "zsoft-npm-outdated-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(
            temp.join("nvm.sh"),
            r#"nvm() { return 0; }
npm() {
    if [ "$ZSOFT_TEST_NPM_CASE" = "updates" ]; then
        printf '%s\n' '{"typescript":{"current":"5.9.0","latest":"6.0.0"}}'
        return 1
    fi
    return 0
}
"#,
        )
        .unwrap();

        for (case, expected) in [("updates", true), ("empty", false)] {
            let output = Command::new("bash")
                .arg("-c")
                .arg(GLOBAL_OUTDATED)
                .env("NVM_DIR", &temp)
                .env("ZSOFT_TEST_NPM_CASE", case)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "npm stdout test {case} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result.get("typescript").is_some(), expected);
        }
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn lifecycle_invalid_dates_fail_closed() {
        let schedule = ReleaseSchedule::parse(BUNDLED_SCHEDULE).unwrap();
        assert_eq!(schedule.classify("v24.1.0", "bad"), NodePhase::Unknown);
    }
}
