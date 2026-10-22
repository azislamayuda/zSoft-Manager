//! Metadata for the lightweight, offline About / Changelog interface.
//! The compiled binary embeds the changelog and GitHub icon; no network needed.

pub const APP_NAME: &str = "zSoft Manager";
pub const LICENSE_NAME: &str = "Personal Use / Noncommercial";
pub const CARGO_VERSION: &str = env!("CARGO_PKG_VERSION");

// Cargo requires SemVer (0.4.2+1), while GitHub release filenames use 0.4.2.1.
// This maps an optional build-metadata revision without changing SemVer parsing.
pub fn release_version() -> String {
    CARGO_VERSION.replace('+', ".")
}
pub const GITHUB_HANDLE: &str = "@azislamayuda";
pub const GITHUB_URL: &str = "https://github.com/azislamayuda/zSoft-Manager";
pub const CHANGELOG: &str = include_str!("../CHANGELOG.md");
pub const GITHUB_ICON: &[u8] = include_bytes!("../assets/github-mark.png");

pub fn version_label() -> String {
    format!("v.{}", release_version())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_read_from_cargo_manifest() {
        assert_eq!(CARGO_VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(release_version(), CARGO_VERSION.replace('+', "."));
        assert_eq!(version_label(), format!("v.{}", release_version()));
    }

    #[test]
    fn release_label_matches_cargo_semver() {
        assert_eq!(CARGO_VERSION, "0.4.6");
        assert_eq!(release_version(), "0.4.6");
    }

    #[test]
    fn github_link_is_the_requested_profile() {
        assert_eq!(GITHUB_HANDLE, "@azislamayuda");
        assert_eq!(GITHUB_URL, "https://github.com/azislamayuda/zSoft-Manager");
        assert!(GITHUB_URL.starts_with("https://"));
    }

    #[test]
    fn embedded_changelog_includes_current_release() {
        assert!(CHANGELOG.contains(&format!("## [{}]", release_version())));
        assert!(CHANGELOG.contains("## [0.2.0]"));
        assert!(CHANGELOG.contains("## [0.1.0]"));
    }

    #[test]
    fn embedded_github_icon_is_a_png() {
        assert!(GITHUB_ICON.len() > 100);
        assert_eq!(&GITHUB_ICON[..8], b"\x89PNG\r\n\x1a\n");
    }
}
