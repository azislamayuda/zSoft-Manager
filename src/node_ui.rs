//! Native GTK4 Node.js management page. No extra window or background daemon.
//! Only the Check updates button accesses remote services.
use crate::node::{self, Install, NodePhase, Versions};
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
    phase: gtk::Label,
    path: gtk::Label,
    default_node: gtk::Label,
    refresh: gtk::Button,
    check: gtk::Button,
    nvm: ToolRow,
    lts: ToolRow,
    npm: ToolRow,
    current: ToolRow,
    installed_rows: gtk::Box,
    global_rows: gtk::Box,
    versions: Versions,
    busy: bool,
}
fn action(label: &str, icon: &str, css: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(css);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    content.set_halign(gtk::Align::Center);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
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
    let available = gtk::Label::new(Some("Latest: Click Check updates"));
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
fn section_title(parent: &gtk::Box, heading: &str, desc: &str) {
    let name = gtk::Label::new(Some(heading));
    name.add_css_class("node-section-heading");
    name.set_xalign(0.0);
    name.set_margin_top(12);
    parent.append(&name);
    let hint = gtk::Label::new(Some(desc));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("pathline");
    parent.append(&hint);
}
fn plain_text(text: &str, css: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(css);
    label
}
pub fn build() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    page.add_css_class("page");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title_column = gtk::Box::new(gtk::Orientation::Vertical, 3);
    title_column.set_hexpand(true);
    title_column.append(&plain_text("Node.js Toolchain", "page-heading"));
    title_column.append(&plain_text(
        "Manage NVM, Node.js versions and global npm packages. Updates are always manual.",
        "subline",
    ));
    header.append(&title_column);
    let refresh = action("Refresh installed", "view-refresh-symbolic", "flat-button");
    refresh.set_valign(gtk::Align::Center);
    let check = action("Check updates", "system-search-symbolic", "soft-button");
    check.set_valign(gtk::Align::Center);
    header.append(&refresh);
    header.append(&check);
    page.append(&header);

    let summary = gtk::Box::new(gtk::Orientation::Vertical, 7);
    summary.add_css_class("node-summary");
    let active_line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let active = plain_text("Active Node: Checking…", "file-title");
    active.set_hexpand(true);
    active_line.append(&active);
    let phase = plain_text("Unknown", "node-phase-badge");
    active_line.append(&phase);
    summary.append(&active_line);
    let default_node = plain_text("NVM default: Checking…", "node-version");
    summary.append(&default_node);
    let path = plain_text("Node path: Checking…", "pathline");
    path.set_selectable(true);
    path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
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
        "Recommended supported release; update sets LTS as default",
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
    section_title(&cards, "Installed Node.js versions", "Set default for new shells, or remove superseded versions of the same major line. Removal is optional and also deletes that version's global npm packages.");
    let installed_rows = gtk::Box::new(gtk::Orientation::Vertical, 7);
    cards.append(&installed_rows);
    section_title(&cards, "Global npm packages", "Installed with the default NVM Node. Check updates first, then update packages individually.");
    let global_rows = gtk::Box::new(gtk::Orientation::Vertical, 7);
    cards.append(&global_rows);
    scroller.set_child(Some(&cards));
    page.append(&scroller);
    let status = plain_text(
        "Reading local Node.js tools. Remote checks are manual.",
        "node-status",
    );
    page.append(&status);

    let model = Rc::new(RefCell::new(Ui {
        status,
        active,
        phase,
        path,
        default_node,
        refresh: refresh.clone(),
        check: check.clone(),
        nvm,
        lts,
        npm,
        current,
        installed_rows,
        global_rows,
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
fn clear(parent: &gtk::Box) {
    while let Some(child) = parent.first_child() {
        parent.remove(&child);
    }
}
fn make_badge(phase: &NodePhase) -> gtk::Label {
    let badge = plain_text(phase.label(), "node-phase-badge");
    badge.add_css_class(phase.css());
    badge.set_tooltip_text(Some(&phase.detail()));
    badge
}
fn update(model: &Rc<RefCell<Ui>>, redraw_lists: bool) {
    // Borrow the snapshot while rendering instead of cloning the release schedule,
    // Node inventory and npm package lists on every UI refresh.
    let m = model.borrow();
    let v = &m.versions;
    let busy = m.busy;
    let installed_rows = &m.installed_rows;
    let global_rows = &m.global_rows;
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
    m.default_node.set_text(&format!(
        "NVM default: {}{}",
        show_value(v.default_node.as_deref()),
        v.default_alias
            .as_deref()
            .map(|s| format!(" (alias: {s})"))
            .unwrap_or_default()
    ));
    let phase = v
        .schedule
        .classify(v.node_active.as_deref().unwrap_or(""), &v.checked_on);
    m.phase.set_text(phase.label());
    for css in [
        "node-badge-lts",
        "node-badge-current",
        "node-badge-eol",
        "node-badge-unknown",
    ] {
        m.phase.remove_css_class(css);
    }
    m.phase.add_css_class(phase.css());
    m.phase.set_tooltip_text(Some(&phase.detail()));
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
    // Disable existing row actions while an operation is running, but keep
    // the widgets allocated until a new snapshot actually arrives.
    installed_rows.set_sensitive(!busy);
    global_rows.set_sensitive(!busy);
    if !redraw_lists {
        return;
    }
    clear(installed_rows);
    if v.installed_nodes.is_empty() {
        installed_rows.append(&plain_text(
            "No NVM-managed Node.js versions found.",
            "node-empty",
        ));
    }
    for entry in &v.installed_nodes {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 11);
        line.add_css_class("node-minicard");
        let title = gtk::Box::new(gtk::Orientation::Vertical, 3);
        title.set_hexpand(true);
        let default = v.default_node.as_deref() == Some(entry.version.as_str());
        let selected = v.node_active.as_deref() == Some(entry.version.as_str());
        let mut title_text = entry.version.clone();
        if default {
            title_text.push_str(" · Default");
        }
        if selected {
            title_text.push_str(" · Active in app");
        }
        title.append(&plain_text(&title_text, "node-version"));
        title.append(&plain_text(&entry.phase.detail(), "pathline"));
        line.append(&title);
        line.append(&make_badge(&entry.phase));
        let switch = action(
            if default { "Default" } else { "Set default" },
            "emblem-default-symbolic",
            "flat-button",
        );
        switch.set_sensitive(!busy && v.nvm_available && !default);
        switch.set_valign(gtk::Align::Center);
        let version = entry.version.clone();
        let switch_model = Rc::clone(model);
        switch.connect_clicked(move |_| set_default(&switch_model, version.clone()));
        line.append(&switch);
        if node::old_node_cleanup_candidate(
            &v.installed_nodes,
            &entry.version,
            v.default_node.as_deref(),
            v.node_active.as_deref(),
        ) {
            let remove = action("Remove old", "user-trash-symbolic", "flat-button");
            remove.set_valign(gtk::Align::Center);
            remove.set_sensitive(!busy && v.nvm_available);
            remove.set_tooltip_text(Some(
                "Remove this superseded NVM version and its version-specific global npm packages",
            ));
            let version = entry.version.clone();
            let model = model.clone();
            remove.connect_clicked(move |_| confirm_remove_old_version(&model, &version));
            line.append(&remove);
        }
        installed_rows.append(&line);
    }
    clear(global_rows);
    if v.global_packages.is_empty() {
        global_rows.append(&plain_text(
            "No global npm packages found for the default NVM Node.",
            "node-empty",
        ));
    }
    for pkg in &v.global_packages {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 11);
        line.add_css_class("node-minicard");
        let detail = gtk::Box::new(gtk::Orientation::Vertical, 3);
        detail.set_hexpand(true);
        detail.append(&plain_text(&pkg.name, "file-title"));
        detail.append(&plain_text(
            &format!(
                "Installed: {}  ·  Latest: {}",
                pkg.installed,
                if v.global_updates_checked {
                    pkg.latest.as_deref().unwrap_or("Up to date")
                } else {
                    "Not checked"
                }
            ),
            "pathline",
        ));
        line.append(&detail);
        let available = pkg.latest.is_some() && pkg.name != "npm";
        let button = action(
            if pkg.name == "npm" {
                "Use npm updater"
            } else {
                "Update"
            },
            "software-update-available-symbolic",
            "soft-button",
        );
        button.set_sensitive(!busy && v.nvm_available && available);
        button.set_valign(gtk::Align::Center);
        if pkg.name == "npm" {
            button.set_tooltip_text(Some("Use the dedicated npm update button above"));
        }
        let name = pkg.name.clone();
        let model = model.clone();
        button.connect_clicked(move |_| update_package(&model, name.clone()));
        line.append(&button);
        global_rows.append(&line);
    }
}

fn probe(model: &Rc<RefCell<Ui>>, remote: bool) {
    if model.borrow().busy {
        return;
    }
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.status.set_text(if remote {
            "Checking official Node.js, NVM and npm sources…"
        } else {
            "Reading locally installed Node.js tools…"
        });
    }
    update(model, false);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(node::inspect(remote));
    });
    let model = model.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || match rx.try_recv() {
        Ok(result) => {
            let mut m = model.borrow_mut();
            m.busy = false;
            let redraw_lists = result.is_ok();
            match result {
                Ok(snapshot) => {
                    let warning = snapshot.warnings.join(" · ");
                    m.versions = snapshot;
                    m.status.set_text(if warning.is_empty() {
                            if remote { "Update check finished. Select a package or Node version to update." }
                            else { "Installed versions refreshed. Click Check updates for online information." }
                        } else { &warning });
                }
                Err(e) => m.status.set_text(&format!("Check failed: {e}")),
            }
            drop(m);
            update(&model, redraw_lists);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            model.borrow_mut().busy = false;
            model
                .borrow()
                .status
                .set_text("Check failed: worker disconnected");
            update(&model, false);
            glib::ControlFlow::Break
        }
    });
}

// Surface NVM failures immediately. The status label at the bottom of the
// scrolling Node page can otherwise be out of view when removing a version.
fn show_operation_error(model: &Rc<RefCell<Ui>>, message: &str) {
    let dialog = gtk::MessageDialog::builder()
        .modal(true)
        .message_type(gtk::MessageType::Error)
        .buttons(gtk::ButtonsType::Close)
        .text("Node.js operation failed")
        .secondary_text(message)
        .build();
    if let Some(window) = model
        .borrow()
        .installed_rows
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    {
        dialog.set_transient_for(Some(&window));
    }
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}

fn operation<F>(model: &Rc<RefCell<Ui>>, progress: &str, job: F)
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    if model.borrow().busy {
        return;
    }
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.status.set_text(progress);
    }
    update(model, false);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result =
            job().and_then(|message| node::inspect(false).map(|versions| (message, versions)));
        let _ = tx.send(result);
    });
    let model = model.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || match rx.try_recv() {
        Ok(result) => {
            let mut m = model.borrow_mut();
            m.busy = false;
            let redraw_lists = result.is_ok();
            let mut error_message = None;
            match result {
                Ok((message, versions)) => {
                    m.versions = versions;
                    m.status.set_text(&message);
                }
                Err(err) => {
                    m.status.set_text(&format!("Operation failed: {err}"));
                    error_message = Some(err);
                }
            }
            drop(m);
            update(&model, redraw_lists);
            if let Some(err) = error_message {
                show_operation_error(&model, &err);
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            model.borrow_mut().busy = false;
            model
                .borrow()
                .status
                .set_text("Operation failed: worker disconnected");
            update(&model, false);
            show_operation_error(&model, "Node.js worker disconnected unexpectedly.");
            glib::ControlFlow::Break
        }
    });
}
fn install(model: &Rc<RefCell<Ui>>, kind: Install) {
    if model.borrow().busy {
        return;
    }
    let (target, allowed) = {
        let model_ref = model.borrow();
        let v = &model_ref.versions;
        // Only clone the one version that the chosen install action needs.
        match kind {
            Install::Nvm => (v.nvm_latest.clone(), v.nvm_git_checkout),
            Install::Lts => (v.lts_latest.clone(), v.nvm_available),
            Install::Npm => (v.npm_latest.clone(), v.nvm_available && v.lts.is_some()),
            Install::Current => (v.current_latest.clone(), v.nvm_available),
        }
    };
    let Some(version) = target.filter(|_| allowed) else {
        return;
    };
    operation(model, "Installing the selected update…", move || {
        node::install(kind, &version)
    });
}
fn confirm_remove_old_version(model: &Rc<RefCell<Ui>>, version: &str) {
    if model.borrow().busy {
        return;
    }
    let dialog = gtk::MessageDialog::builder()
        .modal(true)
        .message_type(gtk::MessageType::Warning)
        .buttons(gtk::ButtonsType::None)
        .text(format!("Remove old Node.js {version}?"))
        .secondary_text("This runs nvm uninstall. The removed version's global npm packages are deleted too. Already-open terminals using that version may need to switch Node.js. Your NVM default and active version are protected.")
        .build();
    if let Some(window) = model
        .borrow()
        .installed_rows
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    {
        dialog.set_transient_for(Some(&window));
    }
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Remove old version", gtk::ResponseType::Accept);
    let model = model.clone();
    let version = version.to_owned();
    dialog.connect_response(move |dialog, response| {
        dialog.close();
        if response == gtk::ResponseType::Accept {
            let requested = version.clone();
            operation(
                &model,
                "Checking NVM safety and removing old Node.js…",
                move || node::remove_old_node_version(&requested),
            );
        }
    });
    dialog.present();
}

fn set_default(model: &Rc<RefCell<Ui>>, version: String) {
    operation(model, "Updating NVM default alias…", move || {
        node::set_default_version(&version)
    });
}
fn update_package(model: &Rc<RefCell<Ui>>, name: String) {
    operation(
        model,
        "Updating the selected global npm package…",
        move || node::update_global_package(&name),
    );
}
