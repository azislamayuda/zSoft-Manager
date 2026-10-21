//! Native GTK4 Node.js tools page. Local inspection runs on page creation;
//! network version checks and installations happen only on button clicks.
use crate::node::{self, Install, Versions};
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc, sync::mpsc, thread, time::Duration};

#[derive(Clone)]
struct ToolRow {
    installed: gtk::Label,
    available: gtk::Label,
    action: gtk::Button,
}

struct Ui {
    status: gtk::Label,
    active: gtk::Label,
    path: gtk::Label,
    refresh: gtk::Button,
    check: gtk::Button,
    nvm: ToolRow,
    lts: ToolRow,
    npm: ToolRow,
    current: ToolRow,
    versions: Versions,
    busy: bool,
}

fn action(label: &str, icon_name: &str, css: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class(css);
    let h = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    h.set_halign(gtk::Align::Center);
    h.append(&gtk::Image::from_icon_name(icon_name));
    h.append(&gtk::Label::new(Some(label)));
    b.set_child(Some(&h));
    b
}

fn tool_row(parent: &gtk::Box, title: &str, subtitle: &str) -> ToolRow {
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    card.add_css_class("node-card");
    let left = gtk::Box::new(gtk::Orientation::Vertical, 5);
    left.set_hexpand(true);
    let name = gtk::Label::new(Some(title));
    name.add_css_class("file-title");
    name.set_xalign(0.0);
    left.append(&name);
    let hint = gtk::Label::new(Some(subtitle));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("pathline");
    left.append(&hint);
    let installed = gtk::Label::new(Some("Installed: Checking…"));
    installed.set_xalign(0.0);
    installed.add_css_class("node-version");
    left.append(&installed);
    let available = gtk::Label::new(Some("Latest: Check updates to find out"));
    available.set_xalign(0.0);
    available.add_css_class("node-latest");
    left.append(&available);
    card.append(&left);
    let update = action(
        "Install update",
        "software-update-available-symbolic",
        "soft-button",
    );
    update.set_valign(gtk::Align::Center);
    update.set_sensitive(false);
    card.append(&update);
    parent.append(&card);
    ToolRow {
        installed,
        available,
        action: update,
    }
}

pub fn build() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    page.add_css_class("page");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title_column = gtk::Box::new(gtk::Orientation::Vertical, 3);
    title_column.set_hexpand(true);
    let heading = gtk::Label::new(Some("Node.js Toolchain"));
    heading.set_xalign(0.0);
    heading.add_css_class("page-heading");
    title_column.append(&heading);
    let description = gtk::Label::new(Some(
        "Manage NVM, Node.js LTS, npm and optional Node.js Current. No automatic cleanup.",
    ));
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.add_css_class("subline");
    title_column.append(&description);
    header.append(&title_column);
    let refresh = action("Refresh installed", "view-refresh-symbolic", "flat-button");
    refresh.set_valign(gtk::Align::Center);
    let check = action("Check updates", "system-search-symbolic", "soft-button");
    check.set_valign(gtk::Align::Center);
    header.append(&refresh);
    header.append(&check);
    page.append(&header);

    let summary = gtk::Box::new(gtk::Orientation::Vertical, 5);
    summary.add_css_class("node-summary");
    let active = gtk::Label::new(Some("Active Node: Checking…"));
    active.set_xalign(0.0);
    active.add_css_class("file-title");
    summary.append(&active);
    let path = gtk::Label::new(Some("Node path: Checking…"));
    path.set_xalign(0.0);
    path.set_selectable(true);
    path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    path.add_css_class("pathline");
    summary.append(&path);
    page.append(&summary);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    let cards = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let nvm = tool_row(&cards, "NVM", "Node Version Manager, installed in ~/.nvm");
    let lts = tool_row(
        &cards,
        "Node.js LTS",
        "Recommended stable release; retained as your default",
    );
    let npm = tool_row(
        &cards,
        "npm",
        "Uses nvm install-latest-npm for Node-compatible updates",
    );
    let current = tool_row(
        &cards,
        "Node.js Current",
        "Optional; installed only after an explicit click",
    );
    scroller.set_child(Some(&cards));
    page.append(&scroller);
    let status = gtk::Label::new(Some(
        "Checking locally installed tools. Remote checks are manual.",
    ));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("node-status");
    page.append(&status);

    let model = Rc::new(RefCell::new(Ui {
        status,
        active,
        path,
        refresh: refresh.clone(),
        check: check.clone(),
        nvm,
        lts,
        npm,
        current,
        versions: Versions::default(),
        busy: false,
    }));
    {
        let model = model.clone();
        refresh.connect_clicked(move |_| probe(&model, false));
    }
    {
        let model = model.clone();
        check.connect_clicked(move |_| probe(&model, true));
    }
    for (kind, button) in [
        (Install::Nvm, model.borrow().nvm.action.clone()),
        (Install::Lts, model.borrow().lts.action.clone()),
        (Install::Npm, model.borrow().npm.action.clone()),
        (Install::Current, model.borrow().current.action.clone()),
    ] {
        let model = model.clone();
        button.connect_clicked(move |_| install(&model, kind));
    }
    probe(&model, false);
    page
}

fn show_value(value: Option<&str>) -> &str {
    value.unwrap_or("Not installed / unavailable")
}

fn set_row(row: &ToolRow, local: Option<&str>, latest: Option<&str>, enabled: bool) {
    row.installed
        .set_text(&format!("Installed: {}", show_value(local)));
    row.available.set_text(&format!(
        "Latest: {}",
        latest.unwrap_or("Not checked / unavailable")
    ));
    row.action.set_sensitive(enabled);
    if let Some(content) = row
        .action
        .child()
        .and_then(|c| c.downcast::<gtk::Box>().ok())
    {
        if let Some(label) = content
            .last_child()
            .and_then(|c| c.downcast::<gtk::Label>().ok())
        {
            label.set_text(if local.is_some() {
                "Install update"
            } else {
                "Install"
            });
        }
    }
}

fn update(model: &Rc<RefCell<Ui>>) {
    let m = model.borrow();
    let v = &m.versions;
    m.refresh.set_sensitive(!m.busy);
    m.check.set_sensitive(!m.busy);
    m.active.set_text(&format!(
        "Active Node: {}",
        show_value(v.node_active.as_deref())
    ));
    m.path.set_text(&format!(
        "Node path: {}",
        show_value(v.node_path.as_deref())
    ));
    set_row(
        &m.nvm,
        v.nvm.as_deref(),
        v.nvm_latest.as_deref(),
        !m.busy && v.nvm_git_checkout && node::newer(v.nvm.as_deref(), v.nvm_latest.as_deref()),
    );
    set_row(
        &m.lts,
        v.lts.as_deref(),
        v.lts_latest.as_deref(),
        !m.busy && v.nvm_available && node::newer(v.lts.as_deref(), v.lts_latest.as_deref()),
    );
    set_row(
        &m.npm,
        v.npm.as_deref(),
        v.npm_latest.as_deref(),
        !m.busy
            && v.nvm_available
            && v.lts.is_some()
            && node::newer(v.npm.as_deref(), v.npm_latest.as_deref()),
    );
    set_row(
        &m.current,
        v.current.as_deref(),
        v.current_latest.as_deref(),
        !m.busy
            && v.nvm_available
            && node::newer(v.current.as_deref(), v.current_latest.as_deref()),
    );
}

fn probe(model: &Rc<RefCell<Ui>>, remote: bool) {
    if model.borrow().busy {
        return;
    }
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.status.set_text(if remote {
            "Checking official upstream versions…"
        } else {
            "Reading local Node.js tools…"
        });
    }
    update(model);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(node::inspect(remote));
    });
    let model = model.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || match rx.try_recv() {
        Ok(result) => {
            let mut m = model.borrow_mut();
            m.busy = false;
            match result {
                Ok(snapshot) => {
                    let warnings = snapshot.warnings.join(" · ");
                    m.versions = snapshot;
                    m.status.set_text(if warnings.is_empty() {
                            if remote { "Update check finished. Choose an enabled button to install." }
                            else { "Installed tool versions refreshed. Click Check updates for remote versions." }
                        } else { &warnings });
                }
                Err(err) => m.status.set_text(&format!("Check failed: {err}")),
            }
            drop(m);
            update(&model);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            model.borrow_mut().busy = false;
            model
                .borrow()
                .status
                .set_text("Check failed: worker disconnected");
            update(&model);
            glib::ControlFlow::Break
        }
    });
}

fn install(model: &Rc<RefCell<Ui>>, kind: Install) {
    if model.borrow().busy {
        return;
    }
    let v = model.borrow().versions.clone();
    let (target, eligible) = match kind {
        Install::Nvm => (v.nvm_latest, v.nvm_git_checkout),
        Install::Lts => (v.lts_latest, v.nvm_available),
        Install::Npm => (v.npm_latest, v.nvm_available && v.lts.is_some()),
        Install::Current => (v.current_latest, v.nvm_available),
    };
    let Some(target) = target.filter(|_| eligible) else {
        return;
    };
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.status
            .set_text("Installing the selected update… Please keep zSoft Manager open.");
    }
    update(model);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = node::install(kind, &target)
            .and_then(|message| node::inspect(false).map(|snapshot| (message, snapshot)));
        let _ = tx.send(result);
    });
    let model = model.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || match rx.try_recv() {
        Ok(result) => {
            let mut m = model.borrow_mut();
            m.busy = false;
            match result {
                Ok((message, versions)) => {
                    m.versions = versions;
                    m.status.set_text(&format!(
                        "{message}. Click Check updates to verify remote versions."
                    ));
                }
                Err(err) => m.status.set_text(&format!("Install failed: {err}")),
            }
            drop(m);
            update(&model);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            model.borrow_mut().busy = false;
            model
                .borrow()
                .status
                .set_text("Install failed: worker disconnected");
            update(&model);
            glib::ControlFlow::Break
        }
    });
}
