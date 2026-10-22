//! English summaries shown as compact toast messages after an explicit update check.
//! Counts represent confirmed update results, not guessed package-version comparisons.

pub fn appimage(available: usize, current: usize, unverified: usize, failed: usize) -> String {
    format!("AppImage: {available} update(s), {current} up to date, {unverified} unverified, {failed} source error(s).")
}

pub fn flatpak(
    app_updates: usize,
    runtime_updates: usize,
    current: usize,
    failed: usize,
) -> String {
    format!("Flatpak: {app_updates} app update(s), {runtime_updates} runtime update(s), {current} up to date, {failed} check error(s).")
}

pub fn node(tool_updates: usize, package_updates: usize, warnings: usize) -> String {
    format!("Node.js: {tool_updates} tool update(s), {package_updates} global npm update(s), {warnings} warning(s).")
}

pub fn python(outdated: usize, installed: usize) -> String {
    format!("Python: {outdated} top-level package update(s) available ({installed} installed).")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_updates_are_reported_explicitly() {
        assert!(appimage(0, 4, 0, 0).contains("0 update(s)"));
        assert!(flatpak(0, 0, 26, 0).contains("0 app update(s), 0 runtime update(s)"));
        assert!(node(0, 0, 0).contains("0 global npm update(s)"));
        assert!(python(0, 10).contains("0 top-level package update(s)"));
    }
    #[test]
    fn failures_and_unverified_are_not_counted_as_current() {
        assert!(appimage(2, 5, 1, 3).contains("1 unverified, 3 source error(s)"));
        assert!(flatpak(2, 1, 20, 2).contains("2 check error(s)"));
        assert!(node(1, 3, 2).contains("2 warning(s)"));
    }
}
