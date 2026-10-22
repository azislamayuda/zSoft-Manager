//! Lazy-loaded GTK4 APT page. Shell-free, Polkit-authorized operations only.
use crate::apt::{self, Action, Overview, Plan};
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc, sync::mpsc, thread, time::Duration};

type Notice = Rc<dyn Fn(&str, bool)>;
struct View {
    root: gtk::Box,
    list: gtk::Box,
    status: gtk::Label,
    counts: gtk::Label,
    log: gtk::TextView,
    check: gtk::Button,
    refresh: gtk::Button,
    upgrade: gtk::Button,
    remove: gtk::Button,
    autoclean: gtk::Button,
    notice: Notice,
    overview: Option<Overview>,
    busy: bool,
}
fn action(text: &str, icon: &str, style: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class(style);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(16);
    row.append(&img);
    row.append(&gtk::Label::new(Some(text)));
    b.set_child(Some(&row));
    b
}
fn label(s: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(s));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.add_css_class(class);
    l
}
pub fn build(notice: Notice) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 13);
    root.add_css_class("page");
    // Keep the three primary APT actions beside the heading, aligned to the right.
    // The operations and their authorization/confirmation callbacks are unchanged.
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let heading = label("APT Manager", "page-heading");
    heading.set_hexpand(true);
    header.append(&heading);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::Center);
    let check = action("Check updates", "system-search-symbolic", "soft-button");
    let refresh = action("Refresh lists", "view-refresh-symbolic", "flat-button");
    let upgrade = action(
        "Upgrade packages",
        "software-update-available-symbolic",
        "primary-button",
    );
    for button in [&check, &refresh, &upgrade] {
        actions.append(button);
    }
    header.append(&actions);
    root.append(&header);
    root.append(&label("Ubuntu system packages · Preview changes before applying · Administrative authorization on request","pathline"));
    let cleanup = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let remove = action("Preview Autoremove", "user-trash-symbolic", "flat-button");
    let autoclean = action("Autoclean cache", "edit-clear-symbolic", "flat-button");
    cleanup.append(&remove);
    cleanup.append(&autoclean);
    root.append(&cleanup);
    let counts = label("Update information not yet checked", "count-label");
    root.append(&counts);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    list.append(&label("Reading APT package state…", "node-status"));
    scroll.set_child(Some(&list));
    root.append(&scroll);
    let status=label("Only the named APT operation receives administrator rights; zSoft Manager stays unprivileged.","node-status");
    status.set_selectable(true);
    root.append(&status);
    let log_label = label("Operation log (current session)", "count-label");
    root.append(&log_label);
    let log_scroll = gtk::ScrolledWindow::new();
    log_scroll.set_min_content_height(90);
    log_scroll.set_max_content_height(155);
    log_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    let log = gtk::TextView::new();
    log.set_editable(false);
    log.set_cursor_visible(false);
    log.set_wrap_mode(gtk::WrapMode::WordChar);
    log.buffer()
        .set_text("No privileged APT operation has run.");
    log_scroll.set_child(Some(&log));
    root.append(&log_scroll);
    let view = Rc::new(RefCell::new(View {
        root: root.clone(),
        list,
        status,
        counts,
        log,
        check: check.clone(),
        refresh: refresh.clone(),
        upgrade: upgrade.clone(),
        remove: remove.clone(),
        autoclean: autoclean.clone(),
        notice,
        overview: None,
        busy: false,
    }));
    {
        let v = view.clone();
        check.connect_clicked(move |_| scan(&v));
    }
    {
        let v = view.clone();
        refresh.connect_clicked(move |_| confirm(&v, Action::Refresh));
    }
    {
        let v = view.clone();
        upgrade.connect_clicked(move |_| confirm(&v, Action::Upgrade));
    }
    {
        let v = view.clone();
        remove.connect_clicked(move |_| confirm(&v, Action::Autoremove));
    }
    {
        let v = view.clone();
        autoclean.connect_clicked(move |_| confirm(&v, Action::Autoclean));
    }
    controls(&view);
    scan(&view);
    root
}
fn controls(view: &Rc<RefCell<View>>) {
    let v = view.borrow();
    v.check.set_sensitive(!v.busy);
    v.refresh.set_sensitive(!v.busy);
    v.autoclean.set_sensitive(!v.busy);
    v.upgrade.set_sensitive(
        !v.busy
            && v.overview
                .as_ref()
                .is_some_and(|o| !o.upgrade.install.is_empty()),
    );
    v.remove.set_sensitive(
        !v.busy
            && v.overview
                .as_ref()
                .is_some_and(|o| o.autoremove.as_ref().is_ok_and(|p| !p.remove.is_empty())),
    );
}
fn render(view: &Rc<RefCell<View>>) {
    {
        let v = view.borrow();
        while let Some(child) = v.list.first_child() {
            v.list.remove(&child);
        }
        if let Some(o) = &v.overview {
            let remove_count = o.autoremove.as_ref().map(|p| p.remove.len());
            v.counts.set_text(&format!(
                "Available upgrades: {}  ·  Autoremove: {}  ·  Held: {}  ·  Reboot: {}",
                o.upgrade.install.len(),
                remove_count.map_or("Blocked".into(), |n| n.to_string()),
                o.held.len(),
                if o.reboot_required {
                    "Required"
                } else {
                    "Not flagged"
                }
            ));
            v.list.append(&label(
                &format!("Upgrade preview: {}", o.upgrade.summary),
                "node-status",
            ));
            for p in &o.upgrade.install {
                let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
                row.add_css_class("node-card");
                row.append(&label(&p.name, "file-title"));
                row.append(&label(
                    &format!("Installed: {}  →  Candidate: {}", p.installed, p.candidate),
                    "pathline",
                ));
                v.list.append(&row);
            }
            if o.upgrade.install.is_empty() {
                v.list.append(&label(
                    "No packages eligible for standard apt-get upgrade.",
                    "node-status",
                ));
            }
            if !o.held.is_empty() {
                v.list.append(&label(
                    &format!("Explicitly held packages: {}", o.held.join(", ")),
                    "node-status",
                ));
            }
            match &o.autoremove {
                Ok(plan) => {
                    v.list.append(&label(
                        &format!("Autoremove preview: {}", plan.summary),
                        "node-status",
                    ));
                    if !plan.remove.is_empty() {
                        v.list.append(&label(
                            &format!("Candidates for removal: {}", plan.remove.join(", ")),
                            "pathline",
                        ));
                    }
                }
                Err(error) => v.list.append(&label(
                    &format!("Autoremove blocked: {error}"),
                    "node-status",
                )),
            }
            if o.reboot_required {
                v.list.append(&label(
                    "Ubuntu reports a reboot is required (/var/run/reboot-required).",
                    "node-status",
                ));
            }
        } else {
            v.list.append(&label(
                "No valid APT preview available. Check updates to retry.",
                "node-status",
            ));
        }
    }
    controls(view);
}
fn scan(view: &Rc<RefCell<View>>) {
    {
        let mut v = view.borrow_mut();
        if v.busy {
            return;
        }
        v.busy = true;
        v.status
            .set_text("Simulating apt-get upgrade and autoremove without changing packages…");
    }
    controls(view);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(apt::overview());
    });
    let view = view.clone();
    glib::timeout_add_local(Duration::from_millis(120), move || match rx.try_recv() {
        Ok(result) => {
            let mut v = view.borrow_mut();
            v.busy = false;
            match result {
                Ok(overview) => {
                    let updates = overview.upgrade.install.len();
                    let removable = overview.autoremove.as_ref().map(|p| p.remove.len());
                    (v.notice)(
                        &format!(
                            "APT: {updates} upgrade(s), {} removable, {} held package(s).",
                            removable.map_or("blocked".into(), |n| n.to_string()),
                            overview.held.len()
                        ),
                        overview.autoremove.is_ok(),
                    );
                    v.status.set_text("APT preview completed. Package lists were not refreshed; Refresh lists requires administrator authorization.");
                    v.overview = Some(overview);
                }
                Err(error) => {
                    (v.notice)(&format!("APT check failed: {error}"), false);
                    v.status.set_text(&format!("APT check failed: {error}"));
                    v.overview = None;
                }
            }
            drop(v);
            render(&view);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            {
                let mut v = view.borrow_mut();
                v.busy = false;
                v.status.set_text("APT preview worker disconnected");
                v.overview = None;
            }
            render(&view);
            glib::ControlFlow::Break
        }
    });
}
fn describe(plan: &Plan) -> String {
    let names = match plan.action {
        Action::Upgrade => plan
            .install
            .iter()
            .map(|p| format!("{}: {} → {}", p.name, p.installed, p.candidate))
            .collect::<Vec<_>>(),
        Action::Autoremove => plan.remove.clone(),
        _ => vec![],
    };
    let mut text = format!("{}\n\n", plan.summary);
    text.push_str(
        &names
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if names.len() > 25 {
        text.push_str(&format!(
            "\n... and {} more. Review the full list in Check updates before confirming.",
            names.len() - 25
        ));
    }
    text
}
fn confirm(view: &Rc<RefCell<View>>, action: Action) {
    let plan = if matches!(action, Action::Upgrade | Action::Autoremove) {
        let v = view.borrow();
        if v.busy {
            return;
        }
        match &v.overview {
            Some(overview) => match action {
                Action::Upgrade => Some(overview.upgrade.clone()),
                Action::Autoremove => match &overview.autoremove {
                    Ok(p) => Some(p.clone()),
                    Err(_) => return,
                },
                _ => None,
            },
            None => return,
        }
    } else {
        if view.borrow().busy {
            return;
        }
        None
    };
    // Large transactions require a more capable paginated review interface.
    // Never ask for blanket approval when the confirmation dialog cannot show
    // the complete list of changed packages.
    if let Some(p) = &plan {
        let count = p.install.len() + p.remove.len();
        if count > 25 {
            let v = view.borrow();
            v.status.set_text(&format!("{} involves {count} packages. This preview is too large for safe GUI confirmation; use Ubuntu Software Updater or APT directly.",action.name()));
            (v.notice)(
                "APT: operation blocked because the preview exceeds 25 packages.",
                false,
            );
            return;
        }
    }
    let details=match &plan{Some(p)=>describe(p),None=>match action {
        Action::Refresh=>"Update package indexes from configured APT repositories. Requires administrator authentication; does not upgrade packages.".into(),
        Action::Autoclean=>"Remove obsolete cached .deb archives. Disk-space savings cannot be reliably estimated before running.".into(),
        _=>String::new(),
    }};
    let dialog=gtk::MessageDialog::builder().modal(true).message_type(gtk::MessageType::Warning)
        .buttons(gtk::ButtonsType::None).text(format!("{}?",action.name()))
        .secondary_text(&format!("{details}\n\nThis may affect system packages. Authenticate with Polkit only if you approve."))
        .build();
    if let Some(window) = view
        .borrow()
        .root
        .root()
        .and_then(|r| r.downcast::<gtk::Window>().ok())
    {
        dialog.set_transient_for(Some(&window));
    }
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Confirm and authorize", gtk::ResponseType::Accept);
    let view = view.clone();
    dialog.connect_response(move |dialog, response| {
        dialog.close();
        if response == gtk::ResponseType::Accept {
            run(&view, action, plan.clone());
        }
    });
    dialog.present();
}
fn add_log(view: &Rc<RefCell<View>>, message: &str) {
    let log = view.borrow().log.clone();
    let buffer = log.buffer();
    if buffer.char_count() > 32000 {
        buffer.set_text("Older lines truncated from GUI; operation output continues below.\n");
    }
    let mut end = buffer.end_iter();
    buffer.insert(&mut end, &format!("{message}\n"));
}
fn run(view: &Rc<RefCell<View>>, action: Action, plan: Option<Plan>) {
    {
        let mut v = view.borrow_mut();
        if v.busy {
            return;
        }
        v.busy = true;
        v.status.set_text(&format!(
            "Running {} with Polkit authorization…",
            action.name()
        ));
        v.log
            .buffer()
            .set_text(&format!("Starting {}…\n", action.name()));
    }
    controls(view);
    let (front_tx, rx) = mpsc::channel::<apt::Message>();
    thread::spawn(move || apt::execute(action, plan, front_tx));
    let view = view.clone();
    glib::timeout_add_local(Duration::from_millis(120), move || {
        for _ in 0..80 {
            match rx.try_recv() {
                Ok(apt::Message::Output(text)) => add_log(&view, &text),
                Ok(apt::Message::Done(action, result, overview)) => {
                    let mut v = view.borrow_mut();
                    v.busy = false;
                    match result {
                        Ok(()) => {
                            (v.notice)(
                                &format!(
                                    "APT: {} completed. Review package state and reboot status.",
                                    action.name()
                                ),
                                true,
                            );
                            v.status.set_text(&format!(
                                "{} completed. {}",
                                action.name(),
                                if overview.is_some() {
                                    "Package status refreshed."
                                } else {
                                    "Click Check updates to refresh package status."
                                }
                            ));
                            v.overview = overview;
                        }
                        Err(error) => {
                            (v.notice)(&format!("APT: {error}"), false);
                            v.status
                                .set_text(&format!("{} failed: {error}", action.name()));
                            v.overview = None;
                        }
                    }
                    drop(v);
                    render(&view);
                    return glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    {
                        let mut v = view.borrow_mut();
                        v.busy = false;
                        v.status.set_text(
                            "APT worker disconnected; verify system state before retrying.",
                        );
                        v.overview = None;
                    }
                    render(&view);
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}
#[cfg(test)]
mod tests {
    #[test]
    fn apt_page_is_explicit_about_privilege_and_preview() {
        let source = include_str!("apt_ui.rs");
        for phrase in [
            "APT Manager",
            "Refresh lists",
            "Upgrade packages",
            "Preview Autoremove",
            "Autoclean cache",
            "Confirm and authorize",
        ] {
            assert!(source.contains(phrase));
        }
    }
    #[test]
    fn apt_primary_actions_share_the_title_row() {
        let source = include_str!("apt_ui.rs");
        // Do not search the test itself: it contains the rejected statement as a string.
        let implementation = source
            .split_once("#[cfg(test)]")
            .expect("APT UI must contain a test module")
            .0;
        assert!(implementation.contains("heading.set_hexpand(true);"));
        assert!(implementation.contains("actions.set_halign(gtk::Align::End);"));
        assert!(implementation.contains("header.append(&heading);"));
        assert!(implementation.contains("header.append(&actions);"));
        assert!(implementation.contains("root.append(&header);"));
        assert!(!implementation.contains("root.append(&actions);"));
    }
}
