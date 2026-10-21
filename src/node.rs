//! NVM / Node.js / npm integration. No persistent service, and no network calls
//! until the user explicitly requests a remote version check or an install.
//! Commands run in a dedicated Bash process because `nvm` is a shell function.
use std::{cmp::Ordering, env, fs, path::PathBuf, process::Command};

#[derive(Clone, Debug, Default)]
pub struct Versions {
    pub nvm: Option<String>,
    pub nvm_latest: Option<String>,
    pub node_active: Option<String>,
    pub node_path: Option<String>,
    pub lts: Option<String>,
    pub lts_latest: Option<String>,
    pub npm: Option<String>,
    pub npm_latest: Option<String>,
    pub current: Option<String>,
    pub current_latest: Option<String>,
    pub nvm_available: bool,
    pub nvm_git_checkout: bool,
    pub warnings: Vec<String>,
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

// A fixed script; no filename, repository URL, or user input is interpolated.
const LOCAL: &str = r#"
set -eo pipefail
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then
    . "$NVM_DIR/nvm.sh" --no-use
    printf 'nvm=%s\n' "$(nvm --version)"
    printf 'lts=%s\n' "$(nvm version 'lts/*' 2>/dev/null || true)"
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
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Command failed: {}", err.trim()));
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

pub fn inspect(check_remote: bool) -> Result<Versions, String> {
    let mut v = parse_local(&bash(LOCAL, None)?);
    v.nvm_git_checkout = nvm_dir().join(".git").is_dir();
    if !check_remote {
        return Ok(v);
    }
    if !v.nvm_available {
        v.warnings
            .push("NVM is not installed. Node.js system packages will not be modified.".into());
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
        Ok(output) if output.status.success() => {
            v.nvm_latest = latest_nvm_tag(&String::from_utf8_lossy(&output.stdout));
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
        let versions = fs::read_dir(nvm_dir().join("versions/node"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|item| item.file_type().is_ok_and(|ft| ft.is_dir()))
            .filter_map(|item| item.file_name().into_string().ok())
            .filter(|name| numbers(name).is_some())
            .collect::<Vec<_>>();
        v.current = current_installed(&versions, latest);
    }
    Ok(v)
}

pub fn install(which: Install, newest: &str) -> Result<String, String> {
    if numbers(newest).is_none() {
        return Err("Invalid version string: installation cancelled".into());
    }
    if !nvm_dir().join("nvm.sh").is_file() {
        return Err("NVM is missing; installation cancelled".into());
    }
    // Explicitly isolate installations to an NVM-managed user environment.
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
nvm install "$1"
nvm alias default 'lts/*'
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
        Install::Current => "Node.js Current installed; LTS remains the default".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare("v20.9.0", "v20.10.0"), Some(Ordering::Less));
        assert!(newer(Some("v1.2.3"), Some("v1.2.4")));
        assert!(!newer(Some("v1.2.4"), Some("v1.2.3")));
        assert!(!newer(Some("v1.2.3"), Some("bad")));
    }
    #[test]
    fn parses_installed_versions() {
        let v = parse_local(
            "nvm=0.40.2\nlts=v24.3.0\nnode=v24.3.0\nnpm=11.4.2\npath=/home/u/.nvm/node\n",
        );
        assert!(v.nvm_available);
        assert_eq!(v.nvm.as_deref(), Some("0.40.2"));
        assert_eq!(v.lts.as_deref(), Some("v24.3.0"));
    }
    #[test]
    fn newest_git_tag() {
        let git = "aaa\trefs/tags/v0.39.0\nbbb\trefs/tags/v0.40.1\nccc\trefs/tags/v0.40.3\n";
        assert_eq!(latest_nvm_tag(git).as_deref(), Some("v0.40.3"));
    }
    #[test]
    fn current_selection_is_major_specific() {
        let versions = vec!["v24.9.0".into(), "v25.1.0".into(), "v25.2.0".into()];
        assert_eq!(
            current_installed(&versions, "v25.3.0").as_deref(),
            Some("v25.2.0")
        );
        assert_eq!(current_installed(&versions, "v26.0.0"), None);
    }
}
