//! Native GTK4 page for an explicitly activated Python venv. Activation applies
//! only to this app's operations, not to an already-open terminal shell.
use crate::{
    check_summary,
    python_env::{self, LocalInfo, PackageUpdate},
};
use gtk::{glib, prelude::*};
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::mpsc, thread, time::Duration};

struct Ui {
    notice: Rc<dyn Fn(&str, bool)>,
    path: gtk::Entry,
    activation: gtk::Button,
    refresh: gtk::Button,
    check: gtk::Button,
    update: gtk::Button,
    select_all: gtk::Button,
    clear: gtk::Button,
    interpreter: gtk::Label,
    pip: gtk::Label,
    count: gtk::Label,
    status: gtk::Label,
    list: gtk::Box,
    rows: Vec<(String, gtk::CheckButton)>,
    updates: Vec<PackageUpdate>,
    folder: Option<PathBuf>,
    active: bool,
    busy: bool,
}

fn action(label: &str, icon: &str, style: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(style);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    content.set_halign(gtk::Align::Center);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}

fn label(container: &gtk::Box, value: &str, class: &str) -> gtk::Label {
    let result = gtk::Label::new(Some(value));
    result.set_xalign(0.0);
    result.set_wrap(true);
    result.add_css_class(class);
    container.append(&result);
    result
}

fn set_action_text(button: &gtk::Button, text: &str) {
    if let Some(content) = button
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    {
        if let Some(label) = content
            .last_child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
        {
            label.set_text(text);
        }
    }
}

pub fn build(notice: Rc<dyn Fn(&str, bool)>) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 13);
    page.add_css_class("page");

    let header = gtk::Box::new(gtk::Orientation::Vertical, 3);
    label(&header, "Python Environment", "page-heading");
    label(&header, "Manage top-level packages in a selected virtual environment. Global Python is never modified.", "subline");
    page.append(&header);

    let path_panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    path_panel.add_css_class("python-config");
    label(&path_panel, "Virtual environment directory", "form-label");
    let path_row = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    let path = gtk::Entry::new();
    path.set_text(&python_env::saved_path());
    path.set_hexpand(true);
    path.add_css_class("form-input");
    path.set_tooltip_text(Some("Default: ~/myenv. You may also enter /home/myenv."));
    let activation = action(
        "Activate",
        "media-playback-start-symbolic",
        "primary-button",
    );
    path_row.append(&path);
    path_row.append(&activation);
    path_panel.append(&path_row);
    label(&path_panel, "Activate/Deactivate controls this application's Python commands only; it does not modify open terminals.", "pathline");
    page.append(&path_panel);

    let summary = gtk::Box::new(gtk::Orientation::Vertical, 5);
    summary.add_css_class("node-summary");
    let interpreter = label(&summary, "Python: Not activated", "file-title");
    let pip = label(&summary, "pip: Not activated", "pathline");
    let count = label(&summary, "Top-level packages: —", "pathline");
    page.append(&summary);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    controls.set_valign(gtk::Align::Center);
    let refresh = action("Refresh installed", "view-refresh-symbolic", "flat-button");
    let check = action("Check updates", "system-search-symbolic", "soft-button");
    let update = action(
        "Update selected",
        "software-update-available-symbolic",
        "primary-button",
    );
    let select_all = action("Select all", "edit-select-all-symbolic", "flat-button");
    let clear = action("Clear", "edit-clear-symbolic", "flat-button");
    for button in [&refresh, &check, &update, &select_all, &clear] {
        controls.append(button);
    }
    page.append(&controls);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    label(
        &list,
        "Activate a Python virtual environment to begin.",
        "node-status",
    );
    scroller.set_child(Some(&list));
    page.append(&scroller);
    let status = label(
        &page,
        "Inactive. No Python subprocess is running.",
        "node-status",
    );

    let ui = Rc::new(RefCell::new(Ui {
        notice,
        path: path.clone(),
        activation: activation.clone(),
        refresh: refresh.clone(),
        check: check.clone(),
        update: update.clone(),
        select_all: select_all.clone(),
        clear: clear.clone(),
        interpreter,
        pip,
        count,
        status,
        list,
        rows: Vec::new(),
        updates: Vec::new(),
        folder: None,
        active: false,
        busy: false,
    }));
    {
        let ui = ui.clone();
        activation.connect_clicked(move |_| toggle_activation(&ui));
    }
    {
        let ui = ui.clone();
        refresh.connect_clicked(move |_| inspect(&ui, false));
    }
    {
        let ui = ui.clone();
        check.connect_clicked(move |_| inspect(&ui, true));
    }
    {
        let ui = ui.clone();
        update.connect_clicked(move |_| upgrade(&ui));
    }
    {
        let ui = ui.clone();
        select_all.connect_clicked(move |_| {
            for (_, button) in &ui.borrow().rows {
                button.set_active(true);
            }
            refresh_controls(&ui);
        });
    }
    {
        let ui = ui.clone();
        clear.connect_clicked(move |_| {
            for (_, button) in &ui.borrow().rows {
                button.set_active(false);
            }
            refresh_controls(&ui);
        });
    }
    refresh_controls(&ui);
    page
}

fn clear_rows(ui: &mut Ui) {
    while let Some(child) = ui.list.first_child() {
        ui.list.remove(&child);
    }
    ui.rows.clear();
    ui.updates.clear();
}

fn refresh_controls(ui: &Rc<RefCell<Ui>>) {
    let model = ui.borrow();
    let ready = model.active && !model.busy;
    model.path.set_sensitive(!model.busy && !model.active);
    model.activation.set_sensitive(!model.busy);
    set_action_text(
        &model.activation,
        if model.active {
            "Deactivate"
        } else {
            "Activate"
        },
    );
    if let Some(content) = model
        .activation
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    {
        if let Some(icon) = content
            .first_child()
            .and_then(|child| child.downcast::<gtk::Image>().ok())
        {
            icon.set_icon_name(Some(if model.active {
                "media-playback-stop-symbolic"
            } else {
                "media-playback-start-symbolic"
            }));
        }
    }
    if model.active {
        model.activation.remove_css_class("primary-button");
        model.activation.add_css_class("soft-button");
    } else {
        model.activation.remove_css_class("soft-button");
        model.activation.add_css_class("primary-button");
    }
    model.refresh.set_sensitive(ready);
    model.check.set_sensitive(ready);
    model
        .update
        .set_sensitive(ready && model.rows.iter().any(|(_, item)| item.is_active()));
    model
        .select_all
        .set_sensitive(ready && !model.rows.is_empty());
    model.clear.set_sensitive(ready && !model.rows.is_empty());
}

fn toggle_activation(ui: &Rc<RefCell<Ui>>) {
    if ui.borrow().busy {
        return;
    }
    if ui.borrow().active {
        let mut model = ui.borrow_mut();
        model.active = false;
        model.folder = None;
        model.interpreter.set_text("Python: Not activated");
        model.pip.set_text("pip: Not activated");
        model.count.set_text("Top-level packages: —");
        model
            .status
            .set_text("Inactive. Other applications and terminal shells are unaffected.");
        clear_rows(&mut model);
        drop(model);
        refresh_controls(ui);
        return;
    }
    let path_text = ui.borrow().path.text().to_string();
    let folder = match python_env::expand_path(
        &path_text,
        &std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    ) {
        Ok(folder) => folder,
        Err(error) => {
            ui.borrow().status.set_text(&error);
            return;
        }
    };
    if let Err(error) = python_env::interpreter(&folder) {
        ui.borrow().status.set_text(&error);
        return;
    }
    {
        let mut model = ui.borrow_mut();
        model.active = true;
        model.folder = Some(folder);
    }
    refresh_controls(ui);
    inspect(ui, false);
}

fn draw_updates(ui: &Rc<RefCell<Ui>>, packages: Vec<PackageUpdate>) {
    {
        let mut model = ui.borrow_mut();
        clear_rows(&mut model);
        if packages.is_empty() {
            label(
                &model.list,
                "All top-level packages are up to date.",
                "node-status",
            );
        }
        model.updates = packages;
    }
    let rows = ui.borrow().updates.clone();
    for item in rows {
        let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        card.add_css_class("node-card");
        let check = gtk::CheckButton::new();
        check.set_active(true); // Match the old zpip workflow: all outdated top-level packages.
        check.set_valign(gtk::Align::Center);
        card.append(&check);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        content.set_hexpand(true);
        label(&content, &item.name, "file-title");
        label(
            &content,
            &format!("{} → {}", item.version, item.latest_version),
            "node-version",
        );
        card.append(&content);
        {
            let mut model = ui.borrow_mut();
            model.list.append(&card);
            model.rows.push((item.name, check.clone()));
        }
        let ui = ui.clone();
        check.connect_toggled(move |_| refresh_controls(&ui));
    }
    refresh_controls(ui);
}

fn inspect(ui: &Rc<RefCell<Ui>>, remote: bool) {
    let folder = {
        let mut model = ui.borrow_mut();
        if !model.active || model.busy {
            return;
        }
        let Some(folder) = model.folder.clone() else {
            return;
        };
        model.busy = true;
        model.status.set_text(if remote {
            "Checking outdated top-level packages…"
        } else {
            "Reading installed Python environment…"
        });
        folder
    };
    refresh_controls(ui);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result: Result<(LocalInfo, Option<Vec<PackageUpdate>>), String> =
            python_env::inspect(&folder).and_then(|info| {
                if remote {
                    python_env::check_updates(&folder).map(|updates| (info, Some(updates)))
                } else {
                    Ok((info, None))
                }
            });
        let _ = tx.send(result);
    });
    let ui = ui.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || {
        match rx.try_recv() {
            Ok(result) => {
                let mut model = ui.borrow_mut();
                model.busy = false;
                match result {
                    Ok((info, updates)) => {
                        model
                            .interpreter
                            .set_text(&format!("Python: {}", info.python));
                        model.pip.set_text(&format!("pip: {}", info.pip));
                        model.count.set_text(&format!(
                            "Installed top-level packages: {}",
                            info.top_level_count
                        ));
                        model.status.set_text(if remote {
                            "Update check finished. Select packages to upgrade."
                        } else {
                            "Environment active. Click Check updates to query PyPI."
                        });
                        if let Err(error) = python_env::save_path(&model.path.text()) {
                            model.status.set_text(&format!(
                                "Environment ready, but settings could not be saved: {error}"
                            ));
                        }
                        if let Some(updates) = updates {
                            (model.notice)(
                                &check_summary::python(updates.len(), info.top_level_count),
                                true,
                            );
                            drop(model);
                            draw_updates(&ui, updates);
                        } else {
                            drop(model);
                        }
                    }
                    Err(error) => {
                        model
                            .status
                            .set_text(&format!("Python check failed: {error}"));
                        if remote {
                            (model.notice)(&format!("Python update check failed: {error}"), false);
                        }
                        if !remote {
                            model.active = false;
                            model.folder = None;
                        }
                        // Release the mutable RefCell borrow before refresh_controls
                        // performs a separate immutable borrow. Otherwise an error
                        // from pip may cause a GTK main-thread panic.
                        drop(model);
                    }
                }
                refresh_controls(&ui);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                ui.borrow_mut().busy = false;
                let model = ui.borrow();
                model
                    .status
                    .set_text("Python check failed: worker disconnected");
                if remote {
                    (model.notice)("Python update check failed: worker disconnected.", false);
                }
                drop(model);
                refresh_controls(&ui);
                glib::ControlFlow::Break
            }
        }
    });
}

fn upgrade(ui: &Rc<RefCell<Ui>>) {
    let (folder, packages, selected) = {
        let mut model = ui.borrow_mut();
        if !model.active || model.busy {
            return;
        }
        let Some(folder) = model.folder.clone() else {
            return;
        };
        let selected: Vec<String> = model
            .rows
            .iter()
            .filter(|(_, check)| check.is_active())
            .map(|(name, _)| name.clone())
            .collect();
        if selected.is_empty() {
            return;
        }
        model.busy = true;
        model.status.set_text(&format!(
            "Upgrading {} selected top-level packages…",
            selected.len()
        ));
        (folder, model.updates.clone(), selected)
    };
    refresh_controls(ui);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = python_env::upgrade(&folder, &selected, &packages);
        let _ = tx.send(result);
    });
    let ui = ui.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || match rx.try_recv() {
        Ok(result) => {
            let mut model = ui.borrow_mut();
            model.busy = false;
            match result {
                Ok(report) => {
                    let suffix = if report.dependency_ok {
                        "pip check: OK."
                    } else {
                        "Warning: pip check reported dependency conflicts."
                    };
                    model.status.set_text(&format!(
                        "Update finished: {} package(s). {} {}",
                        report.count, suffix, report.dependency_message
                    ));
                    clear_rows(&mut model);
                    label(
                        &model.list,
                        "Updates installed. Click Check updates to refresh available versions.",
                        "node-status",
                    );
                }
                Err(error) => model.status.set_text(&format!("Update failed: {error}")),
            }
            drop(model);
            refresh_controls(&ui);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            ui.borrow_mut().busy = false;
            ui.borrow()
                .status
                .set_text("Update failed: worker disconnected");
            refresh_controls(&ui);
            glib::ControlFlow::Break
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn python_page_labels_are_english() {
        let code = include_str!("python_ui.rs");
        for label in [
            "Python Environment",
            "Activate",
            "Deactivate",
            "Check updates",
            "Update selected",
        ] {
            assert!(code.contains(label));
        }
    }
}
