//! Manage one explicitly selected Python virtual environment without touching
//! system Python, launching a shell, or changing the user's terminal session.
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct PackageUpdate {
    pub name: String,
    pub version: String,
    pub latest_version: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Package {
    name: String,
    version: String,
}

#[derive(Clone, Debug, Default)]
pub struct LocalInfo {
    pub python: String,
    pub pip: String,
    pub top_level_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Preferences {
    #[serde(default = "default_display_path")]
    virtualenv: String,
}

fn default_display_path() -> String {
    "~/myenv".to_owned()
}
fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
fn config_file() -> PathBuf {
    home().join(".config/zsoft-manager/python.json")
}

pub fn saved_path() -> String {
    fs::read(config_file())
        .ok()
        .and_then(|data| serde_json::from_slice::<Preferences>(&data).ok())
        .map(|pref| pref.virtualenv)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(default_display_path)
}

pub fn save_path(path: &str) -> Result<(), String> {
    let _ = expand_path(path, &home())?;
    let dest = config_file();
    let parent = dest.parent().ok_or("Invalid preferences directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(&Preferences {
            virtualenv: path.trim().to_owned(),
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, dest).map_err(|e| e.to_string())
}

/// Accept only absolute paths or paths relative to HOME; no shell expansion.
pub fn expand_path(input: &str, home: &Path) -> Result<PathBuf, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter a virtual environment directory.".into());
    }
    let path = if input == "~" {
        home.to_path_buf()
    } else if let Some(tail) = input.strip_prefix("~/") {
        home.join(tail)
    } else {
        let path = PathBuf::from(input);
        if !path.is_absolute() {
            return Err("Use an absolute path or ~/myenv.".into());
        }
        path
    };
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Parent-directory segments (..) are not allowed.".into());
    }
    Ok(path)
}

pub fn interpreter(dir: &Path) -> Result<PathBuf, String> {
    if !dir.join("pyvenv.cfg").is_file() {
        return Err(format!(
            "Not a Python virtual environment: missing {}",
            dir.join("pyvenv.cfg").display()
        ));
    }
    let python = dir.join("bin/python");
    if !python.is_file() {
        return Err(format!(
            "Python interpreter not found: {}",
            python.display()
        ));
    }
    Ok(python)
}

fn output_text(output: Output, action: &str) -> Result<String, String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let reason = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Err(format!("{action} failed: {reason}"))
    }
}

fn run(python: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(python)
        .args(args)
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .env("PIP_DEFAULT_TIMEOUT", "15")
        .env("PIP_RETRIES", "1")
        .output()
        .map_err(|e| format!("Cannot start Python: {e}"))?;
    output_text(output, "Python command")
}

pub fn parse_updates(json: &str) -> Result<Vec<PackageUpdate>, String> {
    let mut packages: Vec<PackageUpdate> =
        serde_json::from_str(json).map_err(|e| format!("Invalid pip update response: {e}"))?;
    packages.retain(|p| valid_package_name(&p.name));
    packages.sort_by_key(|p| p.name.to_ascii_lowercase());
    Ok(packages)
}

pub fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub fn inspect(dir: &Path) -> Result<LocalInfo, String> {
    let python = interpreter(dir)?;
    let venv_flag = run(
        &python,
        &[
            "-c",
            "import sys; print(int(sys.prefix != sys.base_prefix))",
        ],
    )?;
    if venv_flag != "1" {
        return Err("The selected interpreter is not running inside a virtual environment.".into());
    }
    let version = run(&python, &["--version"])?;
    let pip = run(&python, &["-m", "pip", "--version"])?;
    let packages_json = run(
        &python,
        &["-m", "pip", "list", "--not-required", "--format=json"],
    )?;
    let packages: Vec<Package> = serde_json::from_str(&packages_json)
        .map_err(|e| format!("Invalid installed package list: {e}"))?;
    // Touch both fields so a malformed package list is not silently accepted.
    let top_level_count = packages
        .iter()
        .filter(|p| !p.name.is_empty() && !p.version.is_empty())
        .count();
    Ok(LocalInfo {
        python: version,
        pip,
        top_level_count,
    })
}

/// Matches: python -m pip list --outdated --not-required --format=json.
/// The pip subprocess only runs after an explicit Check updates click.
pub fn check_updates(dir: &Path) -> Result<Vec<PackageUpdate>, String> {
    let python = interpreter(dir)?;
    let json = run(
        &python,
        &[
            "-m",
            "pip",
            "list",
            "--outdated",
            "--not-required",
            "--format=json",
        ],
    )?;
    parse_updates(&json)
}

#[derive(Clone, Debug)]
pub struct UpdateReport {
    pub count: usize,
    pub dependency_ok: bool,
    pub dependency_message: String,
}

/// Updates only package names from an already checked, top-level-outdated list.
/// No shell, sudo, global pip, requirements-file or arbitrary command arguments.
pub fn upgrade(
    dir: &Path,
    selected: &[String],
    available: &[PackageUpdate],
) -> Result<UpdateReport, String> {
    let python = interpreter(dir)?;
    let mut packages = Vec::new();
    for name in selected {
        if !valid_package_name(name) || !available.iter().any(|p| p.name.eq_ignore_ascii_case(name))
        {
            return Err(format!(
                "Package is not in the verified outdated list: {name}"
            ));
        }
        if !packages
            .iter()
            .any(|p: &String| p.eq_ignore_ascii_case(name))
        {
            packages.push(name.to_owned());
        }
    }
    if packages.is_empty() {
        return Err("Select at least one outdated top-level package.".into());
    }
    let mut command = Command::new(&python);
    command.args([
        "-m",
        "pip",
        "install",
        "--upgrade",
        "--disable-pip-version-check",
    ]);
    command.args(&packages);
    let output = command
        .env("PIP_DEFAULT_TIMEOUT", "15")
        .env("PIP_RETRIES", "1")
        .output()
        .map_err(|e| format!("Cannot start pip: {e}"))?;
    output_text(output, "Package upgrade")?;
    let check = run(&python, &["-m", "pip", "check"]);
    let (dependency_ok, dependency_message) = match check {
        Ok(message) => (true, message),
        Err(error) => (false, error),
    };
    Ok(UpdateReport {
        count: packages.len(),
        dependency_ok,
        dependency_message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_user_myenv() {
        assert_eq!(
            expand_path("~/myenv", Path::new("/home/test")).unwrap(),
            PathBuf::from("/home/test/myenv")
        );
        assert_eq!(
            expand_path("/home/myenv", Path::new("/home/test")).unwrap(),
            PathBuf::from("/home/myenv")
        );
    }
    #[test]
    fn rejects_relative_and_traversal_paths() {
        assert!(expand_path("myenv", Path::new("/home/test")).is_err());
        assert!(expand_path("~/../etc", Path::new("/home/test")).is_err());
    }
    #[test]
    fn parses_top_level_outdated_json() {
        let data = r#"[{"name":"requests","version":"2.30.0","latest_version":"2.32.5","latest_filetype":"wheel"},{"name":"flask","version":"3.0.0","latest_version":"3.1.0","latest_filetype":"sdist"}]"#;
        let packages = parse_updates(data).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "flask");
        assert_eq!(packages[1].latest_version, "2.32.5");
    }
    #[test]
    fn filters_malicious_package_names() {
        let data = r#"[{"name":"valid-package","version":"1","latest_version":"2"},{"name":"--target=/tmp","version":"1","latest_version":"2"}]"#;
        assert_eq!(parse_updates(data).unwrap().len(), 1);
        for invalid in ["", "-foo", "hello;rm", "foo bar", "@shell"] {
            assert!(!valid_package_name(invalid));
        }
    }
    #[test]
    fn rejects_non_venv_directory_without_running_anything() {
        let result = interpreter(Path::new("/an/unlikely/missing/virtual-environment"));
        assert!(result.is_err());
    }
}
