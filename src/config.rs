use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvider {
    #[default]
    Github,
    KdeKdenlive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    /// Path relative to ~/Applications, e.g. "drawio" or "tools/editor".
    pub directory: String,
    /// GitHub repository, e.g. "owner/project".
    #[serde(default)]
    pub repository: String,
    /// Defaults to GitHub to preserve existing v0.2.3 sources.json files.
    #[serde(default)]
    pub provider: SourceProvider,
    /// Wildcard for already-installed file names in this directory.
    pub installed_pattern: String,
    /// Wildcard for a single release asset, e.g. "*x86_64*.AppImage".
    pub asset_pattern: String,
    /// Stable output basename, e.g. "drawio.AppImage". Old JSON remains compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_filename: Option<String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Sources {
    #[serde(default)]
    pub sources: Vec<Source>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Installed {
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub previous_tags: HashMap<String, String>,
}

pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn applications() -> PathBuf {
    home().join("Applications")
}
pub fn config_path() -> PathBuf {
    home().join(".config/zsoft-manager/sources.json")
}
pub fn installed_path() -> PathBuf {
    home().join(".local/state/zsoft-manager/installed.json")
}
fn old_config_path() -> PathBuf {
    home().join(".config/zappimage-manager/sources.json")
}
fn old_installed_path() -> PathBuf {
    home().join(".local/state/zappimage-manager/installed.json")
}

fn existing_or_legacy(new: PathBuf, legacy: PathBuf) -> PathBuf {
    if new.exists() {
        new
    } else {
        legacy
    }
}

pub fn load_sources() -> Result<Sources, String> {
    load_json(&existing_or_legacy(config_path(), old_config_path()))
}

pub fn save_sources(value: &Sources) -> Result<(), String> {
    save_json(&config_path(), value)
}

pub fn load_installed() -> Result<Installed, String> {
    load_json(&existing_or_legacy(installed_path(), old_installed_path()))
}

pub fn save_installed(value: &Installed) -> Result<(), String> {
    save_json(&installed_path(), value)
}

/// Built-in official KDE source when Kdenlive has not been configured manually.
/// Only applies to the Kdenlive executable, never to other AppImages in its folder.
pub fn built_in_kdenlive(dir: &str, filename: &str) -> Option<Source> {
    if dir.is_empty() || !is_kdenlive_filename(filename) {
        return None;
    }
    Some(Source {
        directory: dir.to_owned(),
        provider: SourceProvider::KdeKdenlive,
        repository: String::new(),
        installed_pattern: "kdenlive*.AppImage".into(),
        asset_pattern: "kdenlive-*-x86_64.AppImage".into(),
        stable_filename: Some("kdenlive.AppImage".into()),
    })
}

pub fn is_kdenlive_filename(filename: &str) -> bool {
    let name = filename.to_ascii_lowercase();
    (name == "kdenlive.appimage" || name.starts_with("kdenlive-")) && name.ends_with(".appimage")
}

pub fn resolved_source(sources: &Sources, dir: &str, filename: &str) -> Option<Source> {
    source_for(sources, dir, filename)
        .cloned()
        .or_else(|| built_in_kdenlive(dir, filename))
}

pub fn source_for<'a>(sources: &'a Sources, dir: &str, file_name: &str) -> Option<&'a Source> {
    sources.sources.iter().find(|s| {
        s.directory == dir
            && (wildmatch(&s.installed_pattern, file_name)
                || s.stable_filename
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(file_name)))
    })
}

pub fn valid_repository(repo: &str) -> bool {
    let pieces: Vec<_> = repo.split('/').collect();
    pieces.len() == 2
        && pieces.iter().all(|s| {
            !s.is_empty()
                && *s != "."
                && *s != ".."
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

/// Supports `*` and `?` only, case insensitive; no regular expressions/shell.
pub fn wildmatch(pattern: &str, input: &str) -> bool {
    let p: Vec<_> = pattern.to_ascii_lowercase().chars().collect();
    let s: Vec<_> = input.to_ascii_lowercase().chars().collect();
    let mut prev = vec![false; s.len() + 1];
    prev[0] = true;
    for pc in p {
        let mut next = vec![false; s.len() + 1];
        if pc == '*' {
            next[0] = prev[0];
        }
        for j in 1..=s.len() {
            next[j] = if pc == '*' {
                next[j - 1] || prev[j]
            } else {
                prev[j - 1] && (pc == '?' || pc == s[j - 1])
            };
        }
        prev = next;
    }
    prev[s.len()]
}

fn load_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn save_json<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let dir = path.parent().ok_or("Invalid configuration path")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".zappimage-{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefers_new_configuration_when_it_exists() {
        let root = std::env::temp_dir().join(format!("zsoft-path-test-{}", std::process::id()));
        let new = root.join("new");
        let legacy = root.join("legacy");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(existing_or_legacy(new.clone(), legacy.clone()), legacy);
        std::fs::write(&new, "{}").unwrap();
        assert_eq!(existing_or_legacy(new.clone(), legacy.clone()), new);
        std::fs::remove_file(new).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn wildcard_matching() {
        assert!(wildmatch(
            "*x86_64*.AppImage",
            "Example-x86_64-2.0.AppImage"
        ));
        assert!(!wildmatch("*x86_64*.AppImage", "Example-aarch64.AppImage"));
        assert!(wildmatch("a?c*", "AbcFOO"));
    }
    #[test]
    fn old_sources_json_is_backward_compatible() {
        let old = r#"{"sources":[{"directory":"drawio","repository":"jgraph/drawio-desktop", "installed_pattern":"*x86_64*.AppImage", "asset_pattern":"*x86_64*.AppImage"}]}"#;
        let mut parsed: Sources = serde_json::from_str(old).unwrap();
        assert!(source_for(&parsed, "drawio", "drawio-x86_64-1.0.AppImage").is_some());
        parsed.sources[0].stable_filename = Some("drawio.AppImage".into());
        assert!(source_for(&parsed, "drawio", "drawio.AppImage").is_some());
    }
    #[test]
    fn old_installed_json_is_backward_compatible() {
        let old: Installed =
            serde_json::from_str(r#"{"tags":{"/tmp/app.AppImage":"v1.0"}}"#).unwrap();
        assert_eq!(old.tags.len(), 1);
        assert!(old.previous_tags.is_empty());
    }
    #[test]
    fn kdenlive_default_source_is_limited_to_its_executable() {
        let source =
            built_in_kdenlive("tools/Kdenlive", "kdenlive-26.08.0-x86_64.AppImage").unwrap();
        assert_eq!(source.provider, SourceProvider::KdeKdenlive);
        assert_eq!(source.stable_filename.as_deref(), Some("kdenlive.AppImage"));
        assert!(built_in_kdenlive("Kdenlive", "other.AppImage").is_none());
        assert!(built_in_kdenlive("Other", "kdenlive.AppImage").is_some());
        assert!(built_in_kdenlive("", "kdenlive.AppImage").is_none());
        assert!(built_in_kdenlive("Kdenlive", "fake-kdenlive.AppImage").is_none());
    }
    #[test]
    fn github_configuration_remains_backward_compatible() {
        let source: Source = serde_json::from_str(r#"{"directory":"drawio","repository":"jgraph/drawio-desktop","installed_pattern":"*.AppImage","asset_pattern":"*.AppImage"}"#).unwrap();
        assert_eq!(source.provider, SourceProvider::Github);
    }
    #[test]
    fn kde_source_roundtrips_without_repository() {
        let source = built_in_kdenlive("Kdenlive", "kdenlive.AppImage").unwrap();
        let decoded: Source =
            serde_json::from_str(&serde_json::to_string(&source).unwrap()).unwrap();
        assert_eq!(decoded.provider, SourceProvider::KdeKdenlive);
    }
    #[test]
    fn safe_repo_validation() {
        assert!(valid_repository("jgraph/drawio-desktop"));
        assert!(!valid_repository("../../x"));
        assert!(!valid_repository("owner/repo/more"));
    }
}

/// Persist the old source mapping plus its new stable filename before a filesystem migration.
/// It is also safe to call when this app has no configured update source.
pub fn ensure_stable_source(dir: &str, current_filename: &str, stable: &str) -> Result<(), String> {
    let mut sources = load_sources()?;
    if let Some(source) = sources.sources.iter_mut().find(|s| {
        s.directory == dir
            && (wildmatch(&s.installed_pattern, current_filename)
                || s.stable_filename
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(current_filename)))
    }) {
        if source.stable_filename.as_deref() != Some(stable) {
            source.stable_filename = Some(stable.to_owned());
            save_sources(&sources)?;
        }
    }
    Ok(())
}
