//! Stable filenames without embedded version/architecture tokens.
use std::path::Path;

pub fn valid_stable_filename(name: &str) -> bool {
    if name.len() > 180 || name.is_empty() || name != name.trim() {
        return false;
    }
    let Some(stem) = name.strip_suffix(".AppImage") else {
        return false;
    };
    !stem.is_empty()
        && stem != "."
        && stem != ".."
        && !stem.starts_with('.')
        && stem
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
}

fn is_version(part: &str) -> bool {
    let text = part
        .strip_prefix('v')
        .or_else(|| part.strip_prefix('V'))
        .unwrap_or(part);
    text.contains('.')
        && text.split('.').count() >= 2
        && text
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

pub fn suggested_stable_filename(original: &Path) -> String {
    let file = original.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = if file.to_ascii_lowercase().ends_with(".appimage") {
        &file[..file.len() - 9]
    } else {
        file
    };
    let mut name = stem.to_owned();
    loop {
        let mut changed = false;
        for suffix in [
            "-x86_64", "_x86_64", "-aarch64", "_aarch64", "-amd64", "_amd64", "-arm64", "_arm64",
            "-x64", "_x64", "-linux", "_linux",
        ] {
            if name.to_ascii_lowercase().ends_with(suffix) {
                name.truncate(name.len() - suffix.len());
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        if let Some(i) = name.rfind(|c: char| matches!(c, '-' | '_' | ' ')) {
            if is_version(&name[i + 1..]) {
                name.truncate(i);
                continue;
            }
        }
        break;
    }
    let name = name.trim_matches(|c: char| matches!(c, '-' | '_' | '.' | ' '));
    let candidate = format!("{name}.AppImage");
    if valid_stable_filename(&candidate) {
        candidate
    } else {
        let folder = original
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("application");
        let sanitized: String = folder
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
            .collect();
        let fallback = format!("{sanitized}.AppImage");
        if valid_stable_filename(&fallback) {
            fallback
        } else {
            "application.AppImage".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_numeric_versions_and_architectures() {
        for (input, expected) in [
            ("drawio-x86_64-32.5.5.AppImage", "drawio.AppImage"),
            ("Whazapp_0.1.5.1_amd64.AppImage", "Whazapp.AppImage"),
            ("Koodo-Reader-2.4.6.AppImage", "Koodo-Reader.AppImage"),
            (
                "GitNative-v1.2.0-linux-x86_64.AppImage",
                "GitNative.AppImage",
            ),
            ("obsidian.AppImage", "obsidian.AppImage"),
        ] {
            assert_eq!(
                suggested_stable_filename(Path::new(input)),
                expected,
                "{input}"
            );
        }
    }
    #[test]
    fn refuses_unsafe_names() {
        for input in [
            "../evil.AppImage",
            "/tmp/file.AppImage",
            "bad\\x.AppImage",
            "bad\nname.AppImage",
            ".hidden.AppImage",
            "test.appimage",
            " .AppImage",
        ] {
            assert!(!valid_stable_filename(input), "{input}");
        }
        assert!(valid_stable_filename("drawio.AppImage"));
    }
}
