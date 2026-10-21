//! Flatpak CLI backend. All commands use argv (never shell interpolation).
//! No remote query runs except after an explicit "Check updates" action.
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    User,
    System,
}
impl Scope {
    pub fn flag(self) -> &'static str {
        match self {
            Self::User => "--user",
            Self::System => "--system",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::System => "System",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    App,
    Runtime,
}
impl Kind {
    pub fn flag(self) -> &'static str {
        match self {
            Self::App => "--app",
            Self::Runtime => "--runtime",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotChecked,
    UpToDate,
    Available,
    CheckFailed,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub id: String,
    pub installed: String,
    pub latest: Option<String>,
    pub branch: String,
    pub origin: String,
    pub arch: String,
    pub active_commit: String,
    pub remote_commit: Option<String>,
    pub scope: Scope,
    pub kind: Kind,
    pub status: Status,
    pub issue: Option<String>,
}
impl Item {
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.scope.label(),
            self.kind.flag(),
            self.id,
            self.arch,
            self.branch
        )
    }
    pub fn reference(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            if self.kind == Kind::App {
                "app"
            } else {
                "runtime"
            },
            self.id,
            self.arch,
            self.branch
        )
    }
}
#[derive(Clone, Debug)]
struct RemoteEntry {
    id: String,
    branch: String,
    arch: String,
    version: String,
    commit: String,
}
fn clean_field(s: &str) -> String {
    s.trim().to_string()
}
fn skip_header(line: &str) -> bool {
    let low = line.trim().to_ascii_lowercase();
    low.starts_with("name\tapplication\t") || low.starts_with("application\tbranch\t")
}
fn parse_local(text: &str, scope: Scope, kind: Kind) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || skip_header(line) {
            continue;
        }
        let fields: Vec<_> = line.split('\t').map(clean_field).collect();
        if fields.len() < 7 {
            return Err(format!(
                "Unexpected flatpak list format on line {}: expected 7 tab-separated columns",
                i + 1
            ));
        }
        if fields[1].is_empty() || fields[3].is_empty() || fields[5].is_empty() {
            continue;
        }
        items.push(Item {
            name: if fields[0].is_empty() {
                fields[1].clone()
            } else {
                fields[0].clone()
            },
            id: fields[1].clone(),
            installed: fields[2].clone(),
            latest: None,
            branch: fields[3].clone(),
            origin: fields[4].clone(),
            arch: fields[5].clone(),
            active_commit: fields[6].clone(),
            remote_commit: None,
            scope,
            kind,
            status: Status::NotChecked,
            issue: None,
        });
    }
    Ok(items)
}
fn parse_remote(text: &str) -> Result<Vec<RemoteEntry>, String> {
    let mut entries = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || skip_header(line) {
            continue;
        }
        let fields: Vec<_> = line.split('\t').map(clean_field).collect();
        if fields.len() < 5 {
            return Err(format!(
                "Unexpected flatpak remote-ls format on line {}: expected 5 tab-separated columns",
                i + 1
            ));
        }
        entries.push(RemoteEntry {
            id: fields[0].clone(),
            branch: fields[1].clone(),
            arch: fields[2].clone(),
            version: fields[3].clone(),
            commit: fields[4].clone(),
        });
    }
    Ok(entries)
}
fn flatpak(args: &[&str]) -> Result<String, String> {
    let output = Command::new("flatpak")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("Could not launch flatpak: {e}. Install Flatpak first if missing."))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(format!(
            "flatpak {} failed: {}",
            args.get(0).copied().unwrap_or(""),
            if detail.is_empty() {
                "Unknown error"
            } else {
                detail
            }
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("Flatpak produced non-UTF-8 output: {e}"))
}
const INSTALLED_COLUMNS: &str = "--columns=name:full,application:full,version:full,branch:full,origin:full,arch:full,active:full";
const REMOTE_COLUMNS: &str =
    "--columns=application:full,branch:full,arch:full,version:full,commit:full";

pub fn installed() -> Result<Vec<Item>, String> {
    let mut all = Vec::new();
    // Explicit scopes avoid confusing user/system installations of the same app.
    for scope in [Scope::User, Scope::System] {
        for kind in [Kind::App, Kind::Runtime] {
            let output = flatpak(&["list", scope.flag(), kind.flag(), INSTALLED_COLUMNS])?;
            all.extend(parse_local(&output, scope, kind)?);
        }
    }
    all.sort_by(|a, b| {
        a.kind.cmp(&b.kind).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    Ok(all)
}
fn remote_key(entry: &RemoteEntry) -> (String, String, String) {
    (entry.id.clone(), entry.branch.clone(), entry.arch.clone())
}
fn match_updates(
    items: &mut [Item],
    scope: Scope,
    kind: Kind,
    origin: &str,
    current: Result<Vec<RemoteEntry>, String>,
    pending: Result<Vec<RemoteEntry>, String>,
) {
    let current_map = current.map(|list| {
        list.into_iter()
            .map(|r| (remote_key(&r), r))
            .collect::<BTreeMap<_, _>>()
    });
    let pending_set = pending.map(|list| {
        list.into_iter()
            .map(|r| remote_key(&r))
            .collect::<BTreeSet<_>>()
    });
    for item in items
        .iter_mut()
        .filter(|item| item.scope == scope && item.kind == kind && item.origin == origin)
    {
        match (&current_map, &pending_set) {
            (Ok(versions), Ok(updates)) => {
                let key = (item.id.clone(), item.branch.clone(), item.arch.clone());
                if let Some(remote) = versions.get(&key) {
                    item.latest = if remote.version.is_empty() || remote.version == "-" {
                        None
                    } else {
                        Some(remote.version.clone())
                    };
                    item.remote_commit = if remote.commit.is_empty() || remote.commit == "-" {
                        None
                    } else {
                        Some(remote.commit.clone())
                    };
                    item.status = if updates.contains(&key) {
                        Status::Available
                    } else {
                        Status::UpToDate
                    };
                    item.issue = None;
                } else {
                    item.status = Status::CheckFailed;
                    item.issue = Some(format!(
                        "No matching remote ref in {origin} ({}/{})",
                        item.arch, item.branch
                    ));
                }
            }
            (Err(e), _) | (_, Err(e)) => {
                item.status = Status::CheckFailed;
                item.issue = Some(e.clone());
            }
        }
    }
}
pub fn check_updates(items: &mut [Item]) {
    let groups: BTreeSet<_> = items
        .iter()
        .filter(|it| !it.origin.trim().is_empty())
        .map(|it| (it.scope, it.kind, it.origin.clone()))
        .collect();
    for item in items.iter_mut().filter(|it| it.origin.trim().is_empty()) {
        item.status = Status::CheckFailed;
        item.issue = Some("Installed Flatpak has no configured origin remote".into());
    }
    for (scope, kind, origin) in groups {
        let current = flatpak(&[
            "remote-ls",
            scope.flag(),
            kind.flag(),
            REMOTE_COLUMNS,
            &origin,
        ])
        .and_then(|s| parse_remote(&s));
        let pending = flatpak(&[
            "remote-ls",
            scope.flag(),
            kind.flag(),
            "--updates",
            REMOTE_COLUMNS,
            &origin,
        ])
        .and_then(|s| parse_remote(&s));
        match_updates(items, scope, kind, &origin, current, pending);
    }
}
fn safe_ref_part(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with('-')
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}
pub fn update_one(item: &Item) -> Result<(), String> {
    if item.status != Status::Available {
        return Err("No confirmed update for this Flatpak ref".into());
    }
    if !safe_ref_part(&item.id) || !safe_ref_part(&item.branch) || !safe_ref_part(&item.arch) {
        return Err("Invalid Flatpak ref; update skipped".into());
    }
    // Flatpak handles authorization for system-wide updates (Polkit). Never run this GUI as root.
    flatpak(&[
        "update",
        item.scope.flag(),
        "--assumeyes",
        "--noninteractive",
        &item.reference(),
    ])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const LOCAL: &str = "Name\tApplication\tVersion\tBranch\tOrigin\tArch\tActive commit\nObsidian\tmd.obsidian.Obsidian\t1.14.4\tstable\tflathub\tx86_64\tlocalabc\n";
    const ALL: &str = "Application\tBranch\tArch\tVersion\tCommit\nmd.obsidian.Obsidian\tstable\tx86_64\t1.14.5\tupstream123\n";
    #[test]
    fn installed_parses_tab_columns() {
        let entries = parse_local(LOCAL, Scope::User, Kind::App).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].installed, "1.14.4");
        assert_eq!(entries[0].scope, Scope::User);
        assert_eq!(
            entries[0].reference(),
            "app/md.obsidian.Obsidian/x86_64/stable"
        );
    }
    #[test]
    fn latest_and_updates_compare_by_ref_not_version() {
        let mut items = parse_local(LOCAL, Scope::User, Kind::App).unwrap();
        match_updates(
            &mut items,
            Scope::User,
            Kind::App,
            "flathub",
            parse_remote(ALL),
            parse_remote(ALL),
        );
        assert_eq!(items[0].status, Status::Available);
        assert_eq!(items[0].latest.as_deref(), Some("1.14.5"));
    }
    #[test]
    fn equal_version_can_still_have_update() {
        let mut items = parse_local(LOCAL, Scope::User, Kind::App).unwrap();
        let same_version = ALL.replace("1.14.5", "1.14.4");
        match_updates(
            &mut items,
            Scope::User,
            Kind::App,
            "flathub",
            parse_remote(&same_version),
            parse_remote(&same_version),
        );
        assert_eq!(items[0].status, Status::Available);
    }
    #[test]
    fn no_pending_update_means_current_even_if_version_metadata_is_different() {
        let mut items = parse_local(LOCAL, Scope::User, Kind::App).unwrap();
        match_updates(
            &mut items,
            Scope::User,
            Kind::App,
            "flathub",
            parse_remote(ALL),
            Ok(vec![]),
        );
        assert_eq!(items[0].status, Status::UpToDate);
    }
    #[test]
    fn user_and_system_never_cross_match() {
        let mut items = parse_local(LOCAL, Scope::System, Kind::App).unwrap();
        match_updates(
            &mut items,
            Scope::User,
            Kind::App,
            "flathub",
            parse_remote(ALL),
            parse_remote(ALL),
        );
        assert_eq!(items[0].status, Status::NotChecked);
    }
    #[test]
    fn missing_remote_ref_is_not_called_current() {
        let mut items = parse_local(LOCAL, Scope::User, Kind::App).unwrap();
        match_updates(
            &mut items,
            Scope::User,
            Kind::App,
            "flathub",
            Ok(vec![]),
            Ok(vec![]),
        );
        assert_eq!(items[0].status, Status::CheckFailed);
    }
    #[test]
    fn network_error_is_not_called_current() {
        let mut items = parse_local(LOCAL, Scope::User, Kind::App).unwrap();
        match_updates(
            &mut items,
            Scope::User,
            Kind::App,
            "flathub",
            Err("offline".into()),
            Ok(vec![]),
        );
        assert_eq!(items[0].status, Status::CheckFailed);
    }
    #[test]
    fn malformed_output_rejected() {
        assert!(parse_local("bad|pipe|output", Scope::User, Kind::App).is_err());
        assert!(parse_remote("bad pipe output").is_err());
    }
    #[test]
    fn command_arguments_never_include_untrusted_ref() {
        let mut item = parse_local(LOCAL, Scope::User, Kind::App)
            .unwrap()
            .remove(0);
        item.status = Status::Available;
        item.branch = "--delete".into();
        assert!(update_one(&item).is_err());
        item.branch = "stable".into();
        item.id = "evil;echo".into();
        assert!(update_one(&item).is_err());
    }
}
