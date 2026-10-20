use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Scans only inside subdirectories, never directly inside the Applications root.
/// Refuses to follow symlinks to avoid escaping the root or ArtCraft exclusions.
pub fn scan(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    if !root.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() && !excluded(&entry.path()) {
            visit(&entry.path(), &mut result)?;
        }
    }
    result.sort();
    Ok(result)
}

fn visit(dir: &Path, result: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            if !excluded(&path) {
                visit(&path, result)?;
            }
        } else if ty.is_file() && is_appimage(&path) {
            result.push(path);
        }
    }
    Ok(())
}

fn excluded(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("ArtCraft"))
}

fn is_appimage(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
}

pub fn relative_dir(root: &Path, path: &Path) -> Option<String> {
    path.parent()?
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn only_scans_subfolders_except_artcraft() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zappimage-test-{}-{id}", std::process::id()));
        for d in [
            "Editor",
            "Editor/inside",
            "ArtCraft",
            "more/ArtCraft",
            "more/sub",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in [
            "root.AppImage",
            "Editor/editor.AppImage",
            "Editor/inside/tool.appimage",
            "ArtCraft/bad.AppImage",
            "more/ArtCraft/no.AppImage",
            "more/sub/yes.AppImage",
            "Editor/note.txt",
        ] {
            fs::write(root.join(f), "test").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(root.join("ArtCraft"), root.join("Editor/link")).unwrap();
            symlink(
                root.join("root.AppImage"),
                root.join("Editor/linked.AppImage"),
            )
            .unwrap();
        }
        let found = scan(&root).unwrap();
        let names: Vec<_> = found
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "Editor/editor.AppImage",
                "Editor/inside/tool.appimage",
                "more/sub/yes.AppImage"
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_root_is_empty() {
        assert!(scan(Path::new("/a/nonexistent/zappimage-test-root-12345"))
            .unwrap()
            .is_empty());
    }
}
