//! On-demand self-update through verified GitHub Release assets.
//! No background timer. A running AppImage must reside in a managed subfolder.
use crate::{
    config::{self, Source, SourceProvider},
    info,
    update::{self, CheckState, ReleaseInfo},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const REPOSITORY: &str = "azislamayuda/zSoft-Manager";
pub const ASSET_PATTERN: &str = "zSoft-Manager-*-x86_64.AppImage";
pub const STABLE_FILENAME: &str = "zSoft-Manager.AppImage";

fn source() -> Source {
    Source {
        directory: "zSoft-Manager".into(),
        provider: SourceProvider::Github,
        repository: REPOSITORY.into(),
        installed_pattern: "zSoft-Manager*.AppImage".into(),
        asset_pattern: ASSET_PATTERN.into(),
        stable_filename: Some(STABLE_FILENAME.into()),
    }
}

pub fn check_release() -> Result<(ReleaseInfo, CheckState), String> {
    // This is a *check* only, so it also works when launched with cargo run.
    // Explicit installed version avoids interpreting filenames as versions.
    update::check(&source(), Path::new(STABLE_FILENAME), Some(info::VERSION))
}

fn allowed_parent(root: &Path, parent: &Path) -> bool {
    parent != root
        && parent.starts_with(root)
        && !parent
            .strip_prefix(root)
            .expect("already checked starts_with")
            .components()
            .any(|component| {
                component
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("ArtCraft")
            })
}

pub fn running_appimage() -> Result<PathBuf, String> {
    let path = std::env::var_os("APPIMAGE")
        .ok_or("Self-update requires the AppImage edition. You are running a development build.")?;
    let path = PathBuf::from(path);
    if !path.is_absolute() || path.extension().and_then(|e| e.to_str()) != Some("AppImage") {
        return Err("APPIMAGE must be an absolute .AppImage path".into());
    }
    if !fs::symlink_metadata(&path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_file()
    {
        return Err("APPIMAGE must point to a regular AppImage file".into());
    }
    let root = fs::canonicalize(config::applications()).map_err(|e| e.to_string())?;
    let parent = fs::canonicalize(path.parent().ok_or("Invalid APPIMAGE path")?)
        .map_err(|e| e.to_string())?;
    if !allowed_parent(&root, &parent) {
        return Err(
            "Install zSoft Manager inside a ~/Applications subfolder to enable safe self-update"
                .into(),
        );
    }
    Ok(path)
}

pub fn install_release(release: &ReleaseInfo) -> Result<String, String> {
    let file = running_appimage()?;
    // update::install validates size/ELF/checksum, backs up and preserves old launcher links.
    update::install(&file, release)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn self_source_is_scoped_to_our_own_release() {
        let s = source();
        assert_eq!(s.repository, REPOSITORY);
        assert_eq!(s.stable_filename.as_deref(), Some(STABLE_FILENAME));
        assert!(config::wildmatch(
            &s.asset_pattern,
            "zSoft-Manager-v0.3.0-x86_64.AppImage"
        ));
        assert!(!config::wildmatch(
            &s.asset_pattern,
            "SomeOtherApp-1.0-x86_64.AppImage"
        ));
        assert!(!config::wildmatch(
            &s.asset_pattern,
            "zSoft-Manager-v0.3.0-aarch64.AppImage"
        ));
    }
    #[test]
    fn self_update_never_targets_applications_root_or_artcraft() {
        let root = Path::new("/tmp/Applications");
        assert!(!allowed_parent(root, root));
        assert!(allowed_parent(root, &root.join("zSoft-Manager")));
        assert!(!allowed_parent(root, &root.join("ArtCraft")));
        assert!(!allowed_parent(root, &root.join("Other/ArtCraft/inside")));
        assert!(!allowed_parent(root, Path::new("/tmp/something-else")));
    }
    #[test]
    fn running_without_appimage_does_not_install() {
        assert!(!Path::new("some.AppImage").is_absolute());
    }
}
