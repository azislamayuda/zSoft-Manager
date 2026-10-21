//! GTK4 Flatpak page. Instantiated lazily; all subprocess work runs off the main thread.
use crate::flatpak::{self, Item, Kind, Status};
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc, sync::mpsc, thread, time::Duration};

struct View {
    list: gtk::Box,
    notice: Rc<dyn Fn(&str, bool)>,
    status: gtk::Label,
    count: gtk::Label,
    rescan: gtk::Button,
    check: gtk::Button,
    update_selected: gtk::Button,
    update_all: gtk::Button,
    select_all: gtk::Button,
    clear: gtk::Button,
    app_tab: gtk::Button,
    runtime_tab: gtk::Button,
    items: Vec<Item>,
    selections: Vec<(String, gtk::CheckButton)>,
    show: Kind,
    busy: bool,
    checked: bool,
}
enum ScanMessage {
    Finished(Result<Vec<Item>, String>, bool),
    Progress(usize, usize, String),
    Updated(Result<Vec<Item>, String>, Vec<String>),
}
fn action(label: &str, icon: &str, style: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(style);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    content.set_halign(gtk::Align::Center);
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(16);
    content.append(&img);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}
fn label(text: &str, style: &str) -> gtk::Label {
    let result = gtk::Label::new(Some(text));
    result.add_css_class(style);
    result.set_xalign(0.0);
    result.set_wrap(true);
    result
}
pub fn build(notice: Rc<dyn Fn(&str, bool)>) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    page.add_css_class("page");
    page.append(&label("Flatpak Manager", "page-heading"));
    page.append(&label("Manage installed Flatpak applications and runtimes from their configured remotes. Updates run only on request.","pathline"));

    let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let app_tab = action(
        "Applications",
        "application-x-executable-symbolic",
        "soft-button",
    );
    let runtime_tab = action("Runtimes", "package-x-generic-symbolic", "flat-button");
    tabs.append(&app_tab);
    tabs.append(&runtime_tab);
    page.append(&tabs);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::Start);
    let rescan = action("Rescan", "view-refresh-symbolic", "flat-button");
    let check = action("Check updates", "system-search-symbolic", "soft-button");
    let select_all = action("Select all", "object-select-symbolic", "flat-button");
    let clear = action("Clear", "edit-clear-symbolic", "flat-button");
    let update_selected = action(
        "Update selected",
        "software-update-available-symbolic",
        "primary-button",
    );
    let update_all = action(
        "Update all",
        "software-update-available-symbolic",
        "soft-button",
    );
    for button in [
        &rescan,
        &check,
        &select_all,
        &clear,
        &update_selected,
        &update_all,
    ] {
        actions.append(button);
    }
    // Controls wrap visually only through GTK flow? Keep horizontal at 1040 px desktop width.
    let action_scroll = gtk::ScrolledWindow::new();
    action_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    action_scroll.set_child(Some(&actions));
    page.append(&action_scroll);
    let count = label("Installed: — · Updates: —", "count-label");
    page.append(&count);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    list.append(&label("Loading installed Flatpaks…", "node-status"));
    scroller.set_child(Some(&list));
    page.append(&scroller);
    let status = label(
        "Reading installed applications and runtimes locally…",
        "node-status",
    );
    status.set_selectable(true);
    page.append(&status);
    let view = Rc::new(RefCell::new(View {
        list,
        notice,
        status,
        count,
        rescan: rescan.clone(),
        check: check.clone(),
        update_selected: update_selected.clone(),
        update_all: update_all.clone(),
        select_all: select_all.clone(),
        clear: clear.clone(),
        app_tab: app_tab.clone(),
        runtime_tab: runtime_tab.clone(),
        items: vec![],
        selections: vec![],
        show: Kind::App,
        busy: false,
        checked: false,
    }));
    {
        let view = view.clone();
        rescan.connect_clicked(move |_| scan(&view, false));
    }
    {
        let view = view.clone();
        check.connect_clicked(move |_| scan(&view, true));
    }
    {
        let view = view.clone();
        app_tab.connect_clicked(move |_| switch(&view, Kind::App));
    }
    {
        let view = view.clone();
        runtime_tab.connect_clicked(move |_| switch(&view, Kind::Runtime));
    }
    {
        let view = view.clone();
        select_all.connect_clicked(move |_| {
            for (_, check) in &view.borrow().selections {
                check.set_active(true);
            }
            controls(&view);
        });
    }
    {
        let view = view.clone();
        clear.connect_clicked(move |_| {
            for (_, check) in &view.borrow().selections {
                check.set_active(false);
            }
            controls(&view);
        });
    }
    {
        let view = view.clone();
        update_selected.connect_clicked(move |_| update(&view, false));
    }
    {
        let view = view.clone();
        update_all.connect_clicked(move |_| update(&view, true));
    }
    controls(&view);
    scan(&view, false);
    page
}
fn switch(view: &Rc<RefCell<View>>, kind: Kind) {
    if view.borrow().busy {
        return;
    }
    view.borrow_mut().show = kind;
    render(view);
}
fn controls(view: &Rc<RefCell<View>>) {
    let v = view.borrow();
    v.rescan.set_sensitive(!v.busy);
    v.check.set_sensitive(!v.busy);
    v.app_tab.set_sensitive(!v.busy);
    v.runtime_tab.set_sensitive(!v.busy);
    if v.show == Kind::App {
        v.app_tab.add_css_class("soft-button");
        v.app_tab.remove_css_class("flat-button");
        v.runtime_tab.add_css_class("flat-button");
        v.runtime_tab.remove_css_class("soft-button");
    } else {
        v.runtime_tab.add_css_class("soft-button");
        v.runtime_tab.remove_css_class("flat-button");
        v.app_tab.add_css_class("flat-button");
        v.app_tab.remove_css_class("soft-button");
    }
    let has_updates = v
        .items
        .iter()
        .any(|item| item.kind == v.show && item.status == Status::Available);
    let selected = v.selections.iter().any(|(_, check)| check.is_active());
    v.update_all.set_sensitive(!v.busy && has_updates);
    v.update_selected.set_sensitive(!v.busy && selected);
    v.select_all.set_sensitive(!v.busy && has_updates);
    v.clear.set_sensitive(!v.busy && selected);
}
fn render(view: &Rc<RefCell<View>>) {
    {
        let mut v = view.borrow_mut();
        while let Some(child) = v.list.first_child() {
            v.list.remove(&child);
        }
        v.selections.clear();
        let rows: Vec<Item> = v
            .items
            .iter()
            .filter(|item| item.kind == v.show)
            .cloned()
            .collect();
        let updates = rows
            .iter()
            .filter(|item| item.status == Status::Available)
            .count();
        let failures = rows
            .iter()
            .filter(|item| item.status == Status::CheckFailed)
            .count();
        v.count.set_text(&format!(
            "Installed: {}  ·  Updates: {}  ·  Check errors: {}",
            rows.len(),
            updates,
            failures
        ));
        if rows.is_empty() {
            v.list.append(&label(
                if v.show == Kind::App {
                    "No Flatpak applications installed."
                } else {
                    "No Flatpak runtimes installed."
                },
                "node-status",
            ));
        }
        for item in &rows {
            let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            card.add_css_class("node-card");
            let select = gtk::CheckButton::new();
            select.set_valign(gtk::Align::Center);
            select.set_sensitive(item.status == Status::Available);
            card.append(&select);
            let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
            content.set_hexpand(true);
            content.append(&label(&item.name, "file-title"));
            content.append(&label(
                &format!(
                    "{} · {} · {} · {}",
                    item.id,
                    item.scope.label(),
                    item.origin,
                    item.branch
                ),
                "pathline",
            ));
            let installed = if item.installed.is_empty() {
                "Unknown"
            } else {
                &item.installed
            };
            content.append(&label(
                &format!(
                    "Installed: {}  ·  Latest: {}",
                    installed,
                    item.latest.as_deref().unwrap_or(if v.checked {
                        "Not available"
                    } else {
                        "Not checked"
                    })
                ),
                "node-version",
            ));
            let abbrev = |value: &str| value.chars().take(12).collect::<String>();
            let local_hash = if item.active_commit.is_empty() {
                "Unknown".to_owned()
            } else {
                abbrev(&item.active_commit)
            };
            let remote_hash = item
                .remote_commit
                .as_deref()
                .map(abbrev)
                .unwrap_or_else(|| "Not checked".to_owned());
            content.append(&label(
                &format!(
                    "Arch: {} · Commit: {} → {}",
                    item.arch, local_hash, remote_hash
                ),
                "pathline",
            ));
            let message = match item.status {
                Status::Available => "Update available".to_owned(),
                Status::UpToDate => "Up to date".to_owned(),
                Status::NotChecked => "Click Check updates".to_owned(),
                Status::CheckFailed => format!(
                    "Check failed: {}",
                    item.issue.as_deref().unwrap_or("Unknown")
                ),
            };
            let text = label(&message, "node-status");
            if item.status == Status::CheckFailed {
                text.set_tooltip_text(item.issue.as_deref());
            }
            content.append(&text);
            card.append(&content);
            v.list.append(&card);
            if item.status == Status::Available {
                v.selections.push((item.key(), select.clone()));
            }
        }
    }
    // Only connect signals after ending the mutable model borrow.
    let checks: Vec<_> = view
        .borrow()
        .selections
        .iter()
        .map(|(_, b)| b.clone())
        .collect();
    for button in checks {
        let view = view.clone();
        button.connect_toggled(move |_| controls(&view));
    }
    controls(view);
}
fn poll(view: &Rc<RefCell<View>>, rx: mpsc::Receiver<ScanMessage>) {
    let view = view.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || match rx.try_recv() {
        Ok(ScanMessage::Progress(done, total, name)) => {
            view.borrow()
                .status
                .set_text(&format!("Updating {done}/{total}: {name}"));
            glib::ControlFlow::Continue
        }
        Ok(ScanMessage::Finished(result, remote)) => {
            {
                let mut v = view.borrow_mut();
                v.busy = false;
                match result {
                    Ok(items) => {
                        (v.notice)(
                            if remote {
                                "Flatpak update check completed"
                            } else {
                                "Flatpak list refreshed"
                            },
                            true,
                        );
                        v.checked = remote;
                        v.items = items;
                        v.status.set_text(if remote {"Update check completed. Only reported Flatpak ref updates are offered."} else {"Installed Flatpaks loaded. Click Check updates to query configured remotes."});
                    }
                    Err(error) => {
                        (v.notice)("Flatpak scan failed", false);
                        v.status.set_text(&format!("Flatpak scan failed: {error}"));
                    }
                }
            }
            render(&view);
            glib::ControlFlow::Break
        }
        Ok(ScanMessage::Updated(result, errors)) => {
            {
                let mut v = view.borrow_mut();
                v.busy = false;
                match result {
                    Ok(items) => {
                        (v.notice)(
                            if errors.is_empty() {
                                "Flatpak updates completed"
                            } else {
                                "Some Flatpak updates failed"
                            },
                            errors.is_empty(),
                        );
                        v.checked = true;
                        v.items = items;
                        v.status.set_text(if errors.is_empty() {
                            "Updates completed. Current Flatpak state refreshed."
                        } else {
                            "Some updates failed. Check the details below and retry if needed."
                        });
                    }
                    Err(error) => {
                        (v.notice)("Flatpak refresh after update failed", false);
                        v.status
                            .set_text(&format!("Could not refresh after updates: {error}"));
                    }
                }
                if !errors.is_empty() {
                    v.status.set_text(&format!(
                        "{} update(s) failed: {}",
                        errors.len(),
                        errors.join(" | ")
                    ));
                }
            }
            render(&view);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            view.borrow_mut().busy = false;
            view.borrow()
                .status
                .set_text("Flatpak worker disconnected before reporting results.");
            controls(&view);
            glib::ControlFlow::Break
        }
    });
}
fn scan(view: &Rc<RefCell<View>>, remote: bool) {
    {
        let mut v = view.borrow_mut();
        if v.busy {
            return;
        }
        v.busy = true;
        v.status.set_text(if remote {
            "Checking Flatpak remotes for updates…"
        } else {
            "Reading installed Flatpaks locally…"
        });
    }
    controls(view);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = flatpak::installed().map(|mut items| {
            if remote {
                flatpak::check_updates(&mut items);
            }
            items
        });
        let _ = tx.send(ScanMessage::Finished(result, remote));
    });
    poll(view, rx);
}
fn update(view: &Rc<RefCell<View>>, all: bool) {
    let targets = {
        let v = view.borrow();
        if v.busy {
            return;
        }
        let selected: Vec<_> = v
            .selections
            .iter()
            .filter(|(_, b)| b.is_active())
            .map(|(key, _)| key.clone())
            .collect();
        v.items
            .iter()
            .filter(|item| {
                item.kind == v.show
                    && item.status == Status::Available
                    && (all || selected.contains(&item.key()))
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    if targets.is_empty() {
        return;
    }
    {
        let mut v = view.borrow_mut();
        v.busy = true;
        v.status
            .set_text(&format!("Updating {} Flatpak ref(s)…", targets.len()));
    }
    controls(view);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut errors = vec![];
        let total = targets.len();
        for (index, item) in targets.iter().enumerate() {
            let _ = tx.send(ScanMessage::Progress(index + 1, total, item.name.clone()));
            if let Err(error) = flatpak::update_one(item) {
                errors.push(format!("{}: {}", item.name, error));
            }
        }
        let result = flatpak::installed().map(|mut items| {
            flatpak::check_updates(&mut items);
            items
        });
        let _ = tx.send(ScanMessage::Updated(result, errors));
    });
    poll(view, rx);
}

#[cfg(test)]
mod tests {
    #[test]
    fn new_flatpak_page_has_separate_runtime_and_app_views() {
        let source = include_str!("flatpak_ui.rs");
        for label in [
            "Flatpak Manager",
            "Applications",
            "Runtimes",
            "Check updates",
            "Update selected",
            "Update all",
        ] {
            assert!(source.contains(label));
        }
    }
}
