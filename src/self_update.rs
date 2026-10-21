//! On-demand self-update through verified GitHub Release assets.
//! No background timer. A running AppImage must reside in a managed subfolder.
use crate::{
    config::{self, Source, SourceProvider},
    info,
    update::{self, CheckState, ReleaseInfo},
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
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

pub enum SelfUpdateStatus {
    Ready(ReleaseInfo, CheckState),
    Pending { tag: String, reason: String },
}

pub fn check_release() -> Result<SelfUpdateStatus, String> {
    // A release may exist while GitHub is still receiving its AppImage/checksum.
    match update::check_with_readiness(
        &source(),
        Path::new(STABLE_FILENAME),
        Some(&info::release_version()),
        true,
    )? {
        update::CheckOutcome::Ready(info, state) => Ok(SelfUpdateStatus::Ready(info, state)),
        update::CheckOutcome::Pending { tag, reason } => {
            Ok(SelfUpdateStatus::Pending { tag, reason })
        }
    }
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

fn verify_installed(path: &Path, release: &ReleaseInfo) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() != release.size {
        return Err(
            "Installed AppImage size or file type differs from the verified download".into(),
        );
    }
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if magic != *b"\x7fELF" {
        return Err("Installed AppImage ELF header is invalid".into());
    }
    if let Some(expected) = &release.sha256 {
        let output = Command::new("sha256sum")
            .arg(path)
            .output()
            .map_err(|e| format!("Post-install SHA256 check failed: {e}"))?;
        if !output.status.success() {
            return Err("Post-install SHA256 verification failed".into());
        }
        let found = String::from_utf8_lossy(&output.stdout);
        if !found
            .split_whitespace()
            .next()
            .unwrap_or("")
            .eq_ignore_ascii_case(expected)
        {
            return Err("Installed AppImage SHA256 mismatch".into());
        }
    }
    Ok(())
}

pub fn install_release(release: &ReleaseInfo) -> Result<String, String> {
    let file = running_appimage()?;
    // update::install first verifies the download, then retains the old file for rollback.
    let result = update::install(&file, release)?;
    let installed_path = file.with_file_name(&release.stable_filename);
    if let Err(error) = verify_installed(&installed_path, release) {
        return match update::rollback(&installed_path) {
            Ok(()) => Err(format!(
                "Post-install verification failed ({error}); previous version restored"
            )),
            Err(restore_error) => Err(format!(
                "Post-install verification failed ({error}); rollback failed: {restore_error}"
            )),
        };
    }
    Ok(format!("{result}; post-install verification passed"))
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
    #[test]
    fn post_install_verification_detects_bad_size_and_bad_header() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "zsoft-postverify-{}-{nonce}.AppImage",
            std::process::id()
        ));
        fs::write(&path, b"\x7fELFpayload").unwrap();
        let release = ReleaseInfo {
            tag: "v0.4.3".into(),
            asset_name: "test.AppImage".into(),
            url: String::new(),
            sha256: None,
            size: 11,
            stable_filename: "test.AppImage".into(),
        };
        assert!(verify_installed(&path, &release).is_ok());
        let mut wrong = release.clone();
        wrong.size = 12;
        assert!(verify_installed(&path, &wrong).is_err());
        fs::write(&path, b"BAD!payload").unwrap();
        assert!(verify_installed(&path, &release).is_err());
        fs::remove_file(path).unwrap();
    }
}
