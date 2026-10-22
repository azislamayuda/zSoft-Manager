//! Local-only dashboard for installed software. Five short-lived inventory
//! workers never perform update checks, write package state, or request root.
//! The dashboard can be refreshed explicitly without leaving the page.
use crate::{config, flatpak, node, python_env, scan};
use gtk::{glib, prelude::*};
use serde_json::Value;
use std::{
    cell::Cell,
    process::Command,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    AppImage,
    Flatpak,
    Apt,
    Node,
    Python,
}
impl Section {
    const ALL: [Section; 5] = [
        Self::AppImage,
        Self::Flatpak,
        Self::Apt,
        Self::Node,
        Self::Python,
    ];
    fn index(self) -> usize {
        match self {
            Self::AppImage => 0,
            Self::Flatpak => 1,
            Self::Apt => 2,
            Self::Node => 3,
            Self::Python => 4,
        }
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    count: usize,
    summary: String,
}
impl Snapshot {
    fn new(count: usize, summary: impl Into<String>) -> Self {
        Self {
            count,
            summary: summary.into(),
        }
    }
}

fn appimage_inventory() -> Result<Snapshot, String> {
    let count = scan::scan(&config::applications())
        .map_err(|e| format!("AppImage scan failed: {e}"))?
        .len();
    Ok(Snapshot::new(
        count,
        "Managed subfolders · ArtCraft excluded",
    ))
}

fn flatpak_inventory() -> Result<Snapshot, String> {
    let all = flatpak::installed()?;
    let apps = all.iter().filter(|i| i.kind == flatpak::Kind::App).count();
    let runtimes = all.len().saturating_sub(apps);
    Ok(Snapshot::new(
        apps,
        format!("Installed applications · {runtimes} runtimes"),
    ))
}

// Count only fully installed packages; do not materialize large name/version
// vectors for the dashboard. This does not imply that APT is up to date.
fn parse_dpkg_count(output: &str) -> usize {
    output
        .lines()
        .filter(|line| {
            let mut fields = line.splitn(3, '\t');
            let name = fields.next().unwrap_or("").trim();
            let version = fields.next().unwrap_or("").trim();
            let state = fields.next().unwrap_or("").trim();
            state.starts_with("ii") && !name.is_empty() && !version.is_empty()
        })
        .count()
}
fn apt_inventory() -> Result<Snapshot, String> {
    let output = Command::new("/usr/bin/dpkg-query")
        .arg("-W")
        .arg(r"-f=${binary:Package}\t${Version}\t${db:Status-Abbrev}\n")
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("Could not read dpkg database: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "dpkg-query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Snapshot::new(
        parse_dpkg_count(&String::from_utf8_lossy(&output.stdout)),
        "Installed Ubuntu packages · local dpkg inventory",
    ))
}

fn node_inventory() -> Result<Snapshot, String> {
    let state = node::inspect(false)?; // Local NVM and npm inventory only.
    let versions = state.installed_nodes.len();
    Ok(Snapshot::new(
        versions,
        format!(
            "Node.js versions · {} global npm packages",
            state.global_packages.len()
        ),
    ))
}

fn python_inventory() -> Result<Snapshot, String> {
    let display_path = python_env::saved_path();
    let path = python_env::expand_path(
        &display_path,
        &std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(".")),
    )?;
    let python = python_env::interpreter(&path)?;
    let out = Command::new(&python)
        .args(["-m", "pip", "list", "--not-required", "--format=json"])
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .env("PIP_DEFAULT_TIMEOUT", "15")
        .env("PIP_RETRIES", "1")
        .output()
        .map_err(|e| format!("Cannot list Python packages: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "Python pip inventory failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let value: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Invalid Python package inventory: {e}"))?;
    let packages = value.as_array().ok_or("Expected a JSON list from pip")?;
    let total = packages
        .iter()
        .filter(|p| {
            p.get("name")
                .and_then(Value::as_str)
                .is_some_and(python_env::valid_package_name)
        })
        .count();
    Ok(Snapshot::new(
        total,
        format!("Top-level Python packages · {display_path}"),
    ))
}

fn scan_section(section: Section) -> Result<Snapshot, String> {
    match section {
        Section::AppImage => appimage_inventory(),
        Section::Flatpak => flatpak_inventory(),
        Section::Apt => apt_inventory(),
        Section::Node => node_inventory(),
        Section::Python => python_inventory(),
    }
}

#[derive(Clone)]
struct CardWidgets {
    count: gtk::Label,
    description: gtk::Label,
}
fn card(section: Section) -> (gtk::Box, CardWidgets) {
    let (heading, icon) = match section {
        Section::AppImage => ("AppImage Manager", "application-x-executable-symbolic"),
        Section::Flatpak => ("Flatpak Manager", "application-x-addon-symbolic"),
        Section::Apt => ("APT Manager", "package-x-generic-symbolic"),
        Section::Node => ("Node.js & npm", "utilities-terminal-symbolic"),
        Section::Python => ("Python & pip", "applications-development-symbolic"),
    };
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    // A compact card must not stretch to fill extra row or viewport height.
    outer.set_valign(gtk::Align::Start);
    outer.set_vexpand(false);
    outer.add_css_class("dashboard-card");
    outer.set_hexpand(true);
    let heading_row = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(20);
    image.add_css_class("dashboard-icon");
    heading_row.append(&image);
    let title = gtk::Label::new(Some(heading));
    title.add_css_class("dashboard-card-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    heading_row.append(&title);
    outer.append(&heading_row);

    let count = gtk::Label::new(Some("—"));
    count.set_xalign(0.0);
    count.add_css_class("dashboard-count");
    outer.append(&count);
    let description = gtk::Label::new(Some("Reading local inventory…"));
    description.add_css_class("dashboard-description");
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_lines(2);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    outer.append(&description);
    (outer, CardWidgets { count, description })
}

fn start_refresh(cards: &[CardWidgets; 5], button: &gtk::Button, status: &gtk::Label) {
    button.set_sensitive(false);
    status.set_text("Reading local inventories · no network update checks");
    for card in cards {
        card.count.set_text("—");
        card.description.set_text("Reading local inventory…");
        card.description.set_tooltip_text(None);
    }
    let (tx, rx): (Sender<(Section, Result<Snapshot, String>)>, Receiver<_>) = mpsc::channel();
    for section in Section::ALL {
        let tx = tx.clone();
        thread::spawn(move || {
            let result = scan_section(section);
            let _ = tx.send((section, result));
        });
    }
    drop(tx);
    let cards = (*cards).clone();
    let button = button.clone();
    let status = status.clone();
    let finished = Rc::new(Cell::new(0_usize));
    glib::timeout_add_local(Duration::from_millis(100), move || {
        loop {
            match rx.try_recv() {
                Ok((section, result)) => {
                    let card = &cards[section.index()];
                    match result {
                        Ok(snapshot) => {
                            card.count.set_text(&snapshot.count.to_string());
                            card.description.set_text(&snapshot.summary);
                        }
                        Err(err) => {
                            card.count.set_text("—");
                            card.description.set_text("Inventory unavailable");
                            let short = if err.chars().count() > 180 {
                                format!("{}…", err.chars().take(180).collect::<String>())
                            } else {
                                err
                            };
                            card.description.set_tooltip_text(Some(&short));
                        }
                    }
                    finished.set(finished.get() + 1);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    button.set_sensitive(true);
                    status.set_text(if finished.get() == Section::ALL.len() {
                        "Local snapshot complete · use each manager to check for updates"
                    } else {
                        "Inventory incomplete · refresh the dashboard to retry"
                    });
                    return glib::ControlFlow::Break;
                }
            }
        }
        if finished.get() >= Section::ALL.len() {
            button.set_sensitive(true);
            status.set_text("Local snapshot complete · use each manager to check for updates");
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn dashboard_columns(is_maximized: bool) -> u32 {
    if is_maximized {
        4
    } else {
        3
    }
}

/// Build once at startup. Loading all inventories is asynchronous and read-only.
pub fn build(window: &gtk::ApplicationWindow) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
    root.add_css_class("page");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("page-header");
    let title = gtk::Label::new(Some("Dashboard"));
    title.add_css_class("page-heading");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    let refresh = gtk::Button::new();
    refresh.add_css_class("soft-button");
    let refresh_content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    refresh_content.append(&gtk::Image::from_icon_name("view-refresh-symbolic"));
    refresh_content.append(&gtk::Label::new(Some("Refresh overview")));
    refresh.set_child(Some(&refresh_content));
    header.append(&refresh);
    root.append(&header);
    let subtitle = gtk::Label::new(Some(
        "Installed software overview · local inventory only · no automatic updates",
    ));
    subtitle.set_xalign(0.0);
    subtitle.add_css_class("body-copy");
    subtitle.set_wrap(true);
    root.append(&subtitle);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    // Precisely three columns normally and four when the GTK window is maximized.
    // Respond to GTK's maximize property rather than running a polling timer.
    let columns = dashboard_columns(window.is_maximized());
    flow.set_min_children_per_line(columns);
    flow.set_max_children_per_line(columns);
    {
        let flow = flow.clone();
        window.connect_notify_local(Some("maximized"), move |window, _| {
            let columns = dashboard_columns(window.is_maximized());
            flow.set_min_children_per_line(columns);
            flow.set_max_children_per_line(columns);
        });
    }
    flow.set_column_spacing(14);
    flow.set_row_spacing(14);
    flow.set_homogeneous(true);
    // GtkScrolledWindow expands, but the cards remain at their natural height.
    // Without START alignment, FlowBox distributes the spare viewport height
    // across its rows, producing unusually tall blank cards.
    flow.set_valign(gtk::Align::Start);
    flow.set_vexpand(false);
    let mut widgets = Vec::new();
    for section in Section::ALL {
        let (item, labels) = card(section);
        flow.insert(&item, -1);
        widgets.push(labels);
    }
    let widgets: [CardWidgets; 5] = widgets
        .try_into()
        .unwrap_or_else(|_| unreachable!("Five inventory cards"));
    scroll.set_child(Some(&flow));
    root.append(&scroll);
    let status = gtk::Label::new(Some("Reading local inventories…"));
    status.set_xalign(0.0);
    status.add_css_class("dashboard-description");
    root.append(&status);
    {
        let widgets = widgets.clone();
        let status = status.clone();
        refresh.connect_clicked(move |btn| start_refresh(&widgets, btn, &status));
    }
    start_refresh(&widgets, &refresh, &status);
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashboard_covers_every_software_manager() {
        assert_eq!(Section::ALL.len(), 5);
        assert_eq!(Section::ALL.map(Section::index), [0, 1, 2, 3, 4]);
    }
    #[test]
    fn installed_dpkg_count_excludes_removed_or_unconfigured() {
        let list = "bash\t5.2\tii \nremoved\t2.0\trc \npart\t3.0\tiU \nlibc6\t2.39\tii \n";
        assert_eq!(parse_dpkg_count(list), 2);
    }
    #[test]
    fn dashboard_cards_only_show_count_and_summary() {
        let production = include_str!("dashboard_ui.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        assert!(!production.contains("Open AppImage"));
        assert!(!production.contains("Open Flatpak"));
        assert!(!production.contains("dashboard-items"));
        assert!(!production.contains("sample_lines("));
        assert!(production.contains("snapshot.summary"));
    }
    #[test]
    fn dashboard_columns_follow_window_maximize_state() {
        let production = include_str!("dashboard_ui.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        assert!(production.contains("dashboard_columns(window.is_maximized())"));
        assert!(production.contains("connect_notify_local(Some(\"maximized\")"));
    }
    #[test]
    fn dashboard_cards_do_not_expand_to_fill_viewport() {
        let production = include_str!("dashboard_ui.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        assert!(production.contains("flow.set_valign(gtk::Align::Start)"));
        assert!(production.contains("flow.set_vexpand(false)"));
        assert!(production.contains("outer.set_valign(gtk::Align::Start)"));
        assert!(production.contains("outer.set_vexpand(false)"));
    }
    #[test]
    fn dashboard_columns_use_three_normal_and_four_maximized() {
        assert_eq!(dashboard_columns(false), 3);
        assert_eq!(dashboard_columns(true), 4);
    }
    #[test]
    fn dashboard_does_not_issue_remote_updates() {
        let source = include_str!("dashboard_ui.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!production.contains("check_updates("));
        assert!(!production.contains("pkexec"));
        assert!(!production.contains("apt-get"));
        assert!(!production.contains("sudo"));
        assert!(production.contains("node::inspect(false)"));
    }
}
