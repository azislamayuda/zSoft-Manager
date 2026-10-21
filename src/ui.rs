use crate::{
    config::{self, Source, SourceProvider},
    info, naming, node_ui, python_ui, scan, self_update,
    update::{self, CheckState, ReleaseInfo},
};
use gtk::{gdk, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
    thread,
    time::Duration,
};

struct AppEntry {
    path: PathBuf,
    status: gtk::Label,
    update_button: gtk::Button,
    source_button: gtk::Button,
    backup_button: gtk::Button,
    normalize_button: gtk::Button,
    release: Option<ReleaseInfo>,
    availability: Option<CheckState>,
}

struct Model {
    root: PathBuf,
    main_panel: gtk::Box,
    modal_layer: gtk::CenterBox,
    list: gtk::ListBox,
    count: gtk::Label,
    footer: gtk::Label,
    refresh: gtk::Button,
    check: gtk::Button,
    update_all: gtk::Button,
    self_button: gtk::Button,
    self_release: Option<ReleaseInfo>,
    toast: gtk::Revealer,
    toast_label: gtk::Label,
    toast_sequence: Rc<Cell<u64>>,
    entries: Vec<AppEntry>,
    busy: bool,
}

enum Event {
    Checked(PathBuf, Result<(ReleaseInfo, CheckState), String>),
    Updated(PathBuf, Result<String, String>),
    SelfChecked(Result<(ReleaseInfo, CheckState), String>),
    SelfInstalled(Result<String, String>),
    Done { refresh_list: bool },
}

// A single GtkStack keeps navigation inside the main window, without dialogs
// for About or Changelog and without any extra runtime or background worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Applications,
    Node,
    Python,
    About,
    Changelog,
}

impl Page {
    fn id(self) -> &'static str {
        match self {
            Self::Applications => "applications",
            Self::Node => "node",
            Self::Python => "python",
            Self::About => "about",
            Self::Changelog => "changelog",
        }
    }
}

const MAIN_ACTION_LABELS: [&str; 3] = ["Rescan", "Check updates", "Update all"];

// All navigation assets are GTK symbolic icons. Using theme icons instead of
// GtkImage::from_paintable keeps Node and Python at the same pixel size as the
// About and Changelog icons, regardless of the original PNG dimensions.
const NAV_SYMBOLIC_ICONS: [&str; 6] = [
    "application-x-executable-symbolic",  // AppImage
    "utilities-terminal-symbolic",        // Node.js
    "applications-development-symbolic",  // Python
    "help-about-symbolic",                // About
    "document-open-recent-symbolic",      // Changelog
    "software-update-available-symbolic", // Self-update
];
const NAV_ICON_SIZE: i32 = 20;
const NAV_BUTTON_SIZE: i32 = 44;
const TOAST_BOTTOM_MARGIN: i32 = 58;

fn button_with_icon(label: &str, icon_name: &str, style: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(style);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(17);
    content.append(&icon);
    let text = gtk::Label::new(Some(label));
    content.append(&text);
    button.set_child(Some(&content));
    button
}

fn set_button_text(button: &gtk::Button, label: &str) {
    if let Some(content) = button
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    {
        if let Some(text) = content
            .last_child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
        {
            text.set_text(label);
        }
    }
}

fn icon_only_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("nav-icon-button");
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(NAV_ICON_SIZE);
    button.set_size_request(NAV_BUTTON_SIZE, NAV_BUTTON_SIZE);
    button.set_child(Some(&icon));
    button.set_tooltip_text(Some(tooltip));
    button
}

#[derive(Clone)]
struct Navigation {
    stack: gtk::Stack,
    appimage: gtk::Button,
    node: gtk::Button,
    python: gtk::Button,
    about: gtk::Button,
    changelog: gtk::Button,
}

fn navigate(nav: &Navigation, page: Page) {
    nav.stack.set_visible_child_name(page.id());
    for (button, destination) in [
        (&nav.appimage, Page::Applications),
        (&nav.node, Page::Node),
        (&nav.python, Page::Python),
        (&nav.about, Page::About),
        (&nav.changelog, Page::Changelog),
    ] {
        if page == destination {
            button.add_css_class("nav-active");
        } else {
            button.remove_css_class("nav-active");
        }
    }
}

fn open_github(status: &gtk::Label) {
    if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
        info::GITHUB_URL,
        None::<&gtk::gio::AppLaunchContext>,
    ) {
        status.set_text(&format!("Could not open GitHub: {error}"));
    }
}

fn detail_row(container: &gtk::Box, label: &str, value: &str) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    row.add_css_class("detail-row");
    let key = gtk::Label::new(Some(label));
    key.add_css_class("detail-key");
    key.set_xalign(0.0);
    key.set_width_chars(16);
    row.append(&key);
    let val = gtk::Label::new(Some(value));
    val.set_xalign(0.0);
    val.set_wrap(true);
    val.set_hexpand(true);
    val.add_css_class("detail-value");
    row.append(&val);
    container.append(&row);
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("io.github.azislamayuda.zsoft-manager")
        .build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &gtk::Application) {
    if let Some(display) = gdk::Display::default() {
        let css = gtk::CssProvider::new();
        css.load_from_data(include_str!("../assets/style.css"));
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("zSoft Manager")
        .default_width(1040)
        .default_height(780)
        .build();

    // GTK-owned CSD headerbar groups the title controls closer together.
    // The system still owns minimize/maximize/close behavior.
    let titlebar = gtk::HeaderBar::new();
    titlebar.set_show_title_buttons(true);
    titlebar.set_decoration_layout(Some(":minimize,maximize,close"));
    titlebar.add_css_class("compact-titlebar");
    window.set_titlebar(Some(&titlebar));

    let root_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root_panel.add_css_class("app-root");
    root_panel.set_margin_top(22);
    root_panel.set_margin_bottom(18);
    root_panel.set_margin_start(26);
    root_panel.set_margin_end(26);

    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    hero.add_css_class("hero");
    let hero_text = gtk::Box::new(gtk::Orientation::Vertical, 5);
    hero_text.set_hexpand(true);
    let headline = gtk::Label::new(Some("zSoft Manager"));
    headline.set_xalign(0.0);
    headline.add_css_class("headline");
    hero_text.append(&headline);
    let subline = gtk::Label::new(Some(
        "Native Rust + GTK4  ·  Stable filenames  ·  Safe updates",
    ));
    subline.set_xalign(0.0);
    subline.add_css_class("subline");
    hero_text.append(&subline);
    hero.append(&hero_text);

    // Flat icon-only navigation; only the active page has a filled background.
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    navigation.set_valign(gtk::Align::Center);
    navigation.add_css_class("navigation");
    let appimage_nav = icon_only_button(NAV_SYMBOLIC_ICONS[0], "AppImage Manager");
    let node_nav = icon_only_button(NAV_SYMBOLIC_ICONS[1], "Node.js / npm / NVM");
    let python_nav = icon_only_button(NAV_SYMBOLIC_ICONS[2], "Python environments / pip");
    let about_nav = icon_only_button(NAV_SYMBOLIC_ICONS[3], "About");
    let changelog_nav = icon_only_button(NAV_SYMBOLIC_ICONS[4], "Changelog");
    navigation.append(&appimage_nav);
    navigation.append(&node_nav);
    navigation.append(&python_nav);
    navigation.append(&about_nav);
    navigation.append(&changelog_nav);
    let self_button = icon_only_button(NAV_SYMBOLIC_ICONS[5], "Check zSoft Manager updates");
    navigation.append(&self_button);
    hero.append(&navigation);
    root_panel.append(&hero);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(150);
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    // APPLICATIONS PAGE: existing AppImage management controls stay intact.
    let apps_page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    apps_page.add_css_class("page");
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.add_css_class("action-bar");
    let refresh = button_with_icon(
        MAIN_ACTION_LABELS[0],
        "view-refresh-symbolic",
        "flat-button",
    );
    let check = button_with_icon(
        MAIN_ACTION_LABELS[1],
        "system-search-symbolic",
        "soft-button",
    );
    let update_all = button_with_icon(
        MAIN_ACTION_LABELS[2],
        "software-update-available-symbolic",
        "primary-button",
    );
    actions.append(&refresh);
    actions.append(&check);
    actions.append(&update_all);
    apps_page.append(&actions);

    let count = gtk::Label::new(None);
    count.set_xalign(0.0);
    count.add_css_class("count-label");
    apps_page.append(&count);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("app-list");
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&list));
    apps_page.append(&scroller);
    stack.add_named(&apps_page, Some(Page::Applications.id()));

    // NODE PAGE: native Rust backend + NVM-managed updates, no background daemon.
    let node_page = node_ui::build();
    stack.add_named(&node_page, Some(Page::Node.id()));

    // PYTHON PAGE: manage one explicit virtual environment; no global pip changes.
    let python_page = python_ui::build();
    stack.add_named(&python_page, Some(Page::Python.id()));

    // ABOUT PAGE: full in-window page, not a dialog or popover.
    let about_page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    about_page.add_css_class("page");
    let about_back = button_with_icon(
        "Back to applications",
        "go-previous-symbolic",
        "back-button",
    );
    about_back.set_halign(gtk::Align::Start);
    about_page.append(&about_back);
    let about_scroll = gtk::ScrolledWindow::new();
    about_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    about_scroll.set_vexpand(true);
    let about_content = gtk::Box::new(gtk::Orientation::Vertical, 15);
    about_content.add_css_class("content-card");
    let about_title = gtk::Label::new(Some("About"));
    about_title.add_css_class("page-heading");
    about_title.set_xalign(0.0);
    about_content.append(&about_title);
    let about_description = gtk::Label::new(Some(
        "A lightweight native software manager for Ubuntu. Manage AppImages from GitHub or KDE, and check or update Node.js LTS, npm and NVM."
    ));
    about_description.set_wrap(true);
    about_description.set_xalign(0.0);
    about_description.add_css_class("body-copy");
    about_content.append(&about_description);
    let details = gtk::Box::new(gtk::Orientation::Vertical, 0);
    details.add_css_class("detail-list");
    detail_row(&details, "Application", info::APP_NAME);
    detail_row(&details, "Version", &info::release_version());
    detail_row(&details, "Developer", "Azis Lamayuda");
    detail_row(&details, "Technology", "Rust · GTK4");
    detail_row(&details, "License", info::LICENSE_NAME);
    detail_row(
        &details,
        "Update source",
        "GitHub Releases · KDE Downloads · nvm-sh · Node.js · npm",
    );
    detail_row(&details, "Network", "On demand only · No background daemon");
    about_content.append(&details);
    let project_link = button_with_icon(
        "Visit GitHub @azislamayuda",
        "web-browser-symbolic",
        "soft-button",
    );
    project_link.set_halign(gtk::Align::Start);
    about_content.append(&project_link);
    about_scroll.set_child(Some(&about_content));
    about_page.append(&about_scroll);
    stack.add_named(&about_page, Some(Page::About.id()));

    // CHANGELOG PAGE: readable offline from the embedded CHANGELOG.md.
    let changelog_page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    changelog_page.add_css_class("page");
    let changelog_back = button_with_icon(
        "Back to applications",
        "go-previous-symbolic",
        "back-button",
    );
    changelog_back.set_halign(gtk::Align::Start);
    changelog_page.append(&changelog_back);
    let changelog_header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    changelog_header.add_css_class("page-header");
    let changelog_heading = gtk::Label::new(Some("Changelog"));
    changelog_heading.set_xalign(0.0);
    changelog_heading.set_hexpand(true);
    changelog_heading.add_css_class("page-heading");
    changelog_header.append(&changelog_heading);
    let current_version = gtk::Label::new(Some(&info::version_label()));
    current_version.add_css_class("version-chip");
    changelog_header.append(&current_version);
    changelog_page.append(&changelog_header);
    let changelog_scroll = gtk::ScrolledWindow::new();
    changelog_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    changelog_scroll.set_vexpand(true);
    changelog_scroll.add_css_class("changelog-scroll");
    let changelog_view = gtk::TextView::new();
    changelog_view.set_editable(false);
    changelog_view.set_cursor_visible(false);
    changelog_view.set_wrap_mode(gtk::WrapMode::WordChar);
    changelog_view.set_top_margin(23);
    changelog_view.set_bottom_margin(23);
    changelog_view.set_left_margin(25);
    changelog_view.set_right_margin(25);
    changelog_view.add_css_class("changelog-text");
    changelog_view.buffer().set_text(info::CHANGELOG);
    changelog_scroll.set_child(Some(&changelog_view));
    changelog_page.append(&changelog_scroll);
    stack.add_named(&changelog_page, Some(Page::Changelog.id()));
    let nav = Navigation {
        stack: stack.clone(),
        appimage: appimage_nav.clone(),
        node: node_nav.clone(),
        python: python_nav.clone(),
        about: about_nav.clone(),
        changelog: changelog_nav.clone(),
    };
    navigate(&nav, Page::Applications);
    root_panel.append(&stack);

    // Fixed footer, visible on every page.
    let footer_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    footer_row.add_css_class("footer-row");
    let footer = gtk::Label::new(Some("Ready · No background network activity"));
    footer.set_xalign(0.0);
    footer.set_hexpand(true);
    footer.set_ellipsize(gtk::pango::EllipsizeMode::End);
    footer.set_tooltip_text(Some("System status"));
    footer.add_css_class("footer");
    footer_row.append(&footer);
    let branding = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    branding.set_valign(gtk::Align::Center);
    let version = gtk::Label::new(Some(&info::version_label()));
    version.add_css_class("version-chip");
    branding.append(&version);
    let github_button = gtk::Button::new();
    github_button.add_css_class("github-link");
    github_button.set_tooltip_text(Some(info::GITHUB_URL));
    let github_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let icon_bytes = glib::Bytes::from_static(info::GITHUB_ICON);
    let github_icon = match gdk::Texture::from_bytes(&icon_bytes) {
        Ok(texture) => gtk::Image::from_paintable(Some(&texture)),
        Err(_) => gtk::Image::from_icon_name("web-browser-symbolic"),
    };
    github_icon.set_pixel_size(16);
    github_content.append(&github_icon);
    github_content.append(&gtk::Label::new(Some(info::GITHUB_HANDLE)));
    github_button.set_child(Some(&github_content));
    branding.append(&github_button);
    footer_row.append(&branding);
    root_panel.append(&footer_row);

    // A real GtkOverlay child, not GtkDialog or a second GtkWindow:
    // the Ubuntu Dock continues to see only the main application window.
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&root_panel));
    let modal_layer = gtk::CenterBox::new();
    modal_layer.set_orientation(gtk::Orientation::Vertical);
    modal_layer.set_hexpand(true);
    modal_layer.set_vexpand(true);
    modal_layer.set_halign(gtk::Align::Fill);
    modal_layer.set_valign(gtk::Align::Fill);
    modal_layer.add_css_class("modal-backdrop");
    modal_layer.set_visible(false);
    // Intercept clicks on the backdrop; underlying content is also insensitive.
    let backdrop_guard = gtk::GestureClick::new();
    backdrop_guard.connect_pressed(|_, _, _, _| {});
    modal_layer.add_controller(backdrop_guard);
    overlay.add_overlay(&modal_layer);

    // Non-blocking toast notifications share the main GtkWindow; no extra Dock indicator.
    let toast = gtk::Revealer::new();
    toast.set_transition_type(gtk::RevealerTransitionType::SlideUp);
    toast.set_transition_duration(220);
    toast.set_halign(gtk::Align::Center);
    toast.set_valign(gtk::Align::End);
    // Show toasts above the bottom footer, horizontally centered.
    toast.set_margin_bottom(TOAST_BOTTOM_MARGIN);
    toast.set_reveal_child(false);
    let toast_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    toast_box.add_css_class("toast");
    let toast_icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    toast_box.append(&toast_icon);
    let toast_label = gtk::Label::new(None);
    toast_label.set_max_width_chars(54);
    toast_label.set_wrap(true);
    toast_box.append(&toast_label);
    toast.set_child(Some(&toast_box));
    overlay.add_overlay(&toast);
    window.set_child(Some(&overlay));

    for (page, button) in [
        (Page::Applications, appimage_nav.clone()),
        (Page::Node, node_nav.clone()),
        (Page::Python, python_nav.clone()),
        (Page::About, about_nav.clone()),
        (Page::Changelog, changelog_nav.clone()),
    ] {
        let nav = nav.clone();
        button.connect_clicked(move |_| navigate(&nav, page));
    }
    for back in [about_back, changelog_back] {
        let nav = nav.clone();
        back.connect_clicked(move |_| navigate(&nav, Page::Applications));
    }
    {
        let status = footer.clone();
        github_button.connect_clicked(move |_| open_github(&status));
    }
    {
        let status = footer.clone();
        project_link.connect_clicked(move |_| open_github(&status));
    }

    let model = Rc::new(RefCell::new(Model {
        root: config::applications(),
        main_panel: root_panel.clone(),
        modal_layer: modal_layer.clone(),
        list,
        count,
        footer,
        refresh: refresh.clone(),
        check: check.clone(),
        update_all: update_all.clone(),
        self_button: self_button.clone(),
        self_release: None,
        toast,
        toast_label,
        toast_sequence: Rc::new(Cell::new(0)),
        entries: vec![],
        busy: false,
    }));
    {
        let model = model.clone();
        refresh.connect_clicked(move |_| rescan(&model));
    }
    {
        let model = model.clone();
        check.connect_clicked(move |_| check_updates(&model));
    }
    {
        let model = model.clone();
        update_all.connect_clicked(move |_| {
            let updates = model
                .borrow()
                .entries
                .iter()
                .filter(|entry| entry.availability == Some(CheckState::Available))
                .filter_map(|entry| {
                    entry
                        .release
                        .clone()
                        .map(|release| (entry.path.clone(), release))
                })
                .collect();
            launch_update(&model, updates);
        });
    }
    {
        let model = model.clone();
        self_button.connect_clicked(move |_| {
            let release = model.borrow().self_release.clone();
            if let Some(release) = release {
                launch_self_update(&model, release);
            } else {
                check_self_update(&model);
            }
        });
    }
    // Escape dismisses the embedded panel without destroying the main window.
    {
        let esc_model = model.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape && esc_model.borrow().modal_layer.is_visible() {
                close_modal(&esc_model);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        modal_layer.add_controller(key_controller);
    }
    rescan(&model);
    window.present();
}

#[derive(Clone, Copy)]
enum Notice {
    Success,
    Info,
    Error,
}

fn notify(model: &Rc<RefCell<Model>>, kind: Notice, message: &str) {
    let (toast, label, sequence) = {
        let m = model.borrow();
        (
            m.toast.clone(),
            m.toast_label.clone(),
            m.toast_sequence.clone(),
        )
    };
    toast.remove_css_class("toast-success");
    toast.remove_css_class("toast-info");
    toast.remove_css_class("toast-error");
    toast.add_css_class(match kind {
        Notice::Success => "toast-success",
        Notice::Info => "toast-info",
        Notice::Error => "toast-error",
    });
    label.set_text(message);
    toast.set_reveal_child(true);
    let ticket = sequence.get().wrapping_add(1);
    sequence.set(ticket);
    glib::timeout_add_local_once(Duration::from_secs(5), move || {
        if sequence.get() == ticket {
            toast.set_reveal_child(false);
        }
    });
}

fn check_self_update(model: &Rc<RefCell<Model>>) {
    if model.borrow().busy {
        return;
    }
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.self_release = None;
        m.footer
            .set_text("Checking zSoft Manager releases on GitHub…");
    }
    notify(model, Notice::Info, "Checking zSoft Manager updates…");
    update_control_state(model);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(Event::SelfChecked(self_update::check_release()));
        let _ = tx.send(Event::Done {
            refresh_list: false,
        });
    });
    receive_events(model.clone(), rx);
}

fn launch_self_update(model: &Rc<RefCell<Model>>, release: ReleaseInfo) {
    if model.borrow().busy {
        return;
    }
    // Only a running AppImage under ~/Applications/<subfolder> can install itself.
    if let Err(reason) = self_update::running_appimage() {
        notify(model, Notice::Error, &reason);
        return;
    }
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.footer.set_text("Downloading zSoft Manager update…");
    }
    notify(
        model,
        Notice::Info,
        "Downloading and verifying zSoft Manager…",
    );
    update_control_state(model);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(Event::SelfInstalled(self_update::install_release(&release)));
        let _ = tx.send(Event::Done {
            refresh_list: false,
        });
    });
    receive_events(model.clone(), rx);
}

fn rescan(model: &Rc<RefCell<Model>>) {
    if model.borrow().busy {
        return;
    }
    let root = model.borrow().root.clone();
    let scan_result = scan::scan(&root);
    let files = match scan_result {
        Ok(files) => files,
        Err(e) => {
            model
                .borrow()
                .footer
                .set_text(&format!("Could not read folder: {e}"));
            notify(model, Notice::Error, &format!("Scan failed: {e}"));
            return;
        }
    };
    {
        let mut m = model.borrow_mut();
        while let Some(row) = m.list.first_child() {
            m.list.remove(&row);
        }
        m.entries.clear();
        m.count.set_text(&format!(
            "{} AppImages found · Subfolders only · ArtCraft excluded",
            files.len()
        ));
    }
    for path in files {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        line.set_margin_top(13);
        line.set_margin_bottom(13);
        line.set_margin_start(17);
        line.set_margin_end(17);
        let detail = gtk::Box::new(gtk::Orientation::Vertical, 5);
        detail.set_hexpand(true);
        let name = path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new(""))
            .to_string_lossy();
        let title = gtk::Label::new(Some(&name));
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("file-title");
        let pathline = gtk::Label::new(Some(
            &path.strip_prefix(&root).unwrap_or(&path).to_string_lossy(),
        ));
        pathline.set_xalign(0.0);
        pathline.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        pathline.add_css_class("pathline");
        let status = gtk::Label::new(Some("Not checked yet"));
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.set_tooltip_text(Some("Click Check updates to query the configured provider"));
        status.add_css_class("status-text");
        detail.append(&title);
        detail.append(&pathline);
        detail.append(&status);
        line.append(&detail);

        let source_button = button_with_icon("Source", "emblem-system-symbolic", "subtle-button");
        let normalize_button =
            button_with_icon("Stable name", "edit-rename-symbolic", "subtle-button");
        let backup_button = button_with_icon("Rollback", "edit-undo-symbolic", "subtle-button");
        let update_button = button_with_icon(
            "Install update",
            "software-update-available-symbolic",
            "soft-button",
        );
        line.append(&source_button);
        line.append(&normalize_button);
        line.append(&update_button);
        line.append(&backup_button);
        let row = gtk::ListBoxRow::new();
        row.add_css_class("app-row");
        row.set_child(Some(&line));
        let path_for_normalize = path.clone();
        {
            let model = model.clone();
            normalize_button
                .connect_clicked(move |_| confirm_normalization(&model, &path_for_normalize));
        }
        let path_for_source = path.clone();
        let path_for_update = path.clone();
        let path_for_rollback = path.clone();
        {
            let model = model.clone();
            source_button.connect_clicked(move |_| configure_source(&model, &path_for_source));
        }
        {
            let model = model.clone();
            update_button.connect_clicked(move |_| {
                let chosen = model
                    .borrow()
                    .entries
                    .iter()
                    .find(|e| e.path == path_for_update)
                    .and_then(|e| e.release.clone());
                if let Some(release) = chosen {
                    launch_update(&model, vec![(path_for_update.clone(), release)]);
                }
            });
        }
        {
            let model = model.clone();
            backup_button.connect_clicked(move |_| {
                if model.borrow().busy {
                    return;
                }
                match update::rollback(&path_for_rollback) {
                    Ok(()) => {
                        if let Some(entry) = model
                            .borrow_mut()
                            .entries
                            .iter_mut()
                            .find(|e| e.path == path_for_rollback)
                        {
                            entry.release = None;
                            entry.availability = None;
                            entry
                                .status
                                .set_text("Rollback complete · Check for updates again");
                        }
                        model.borrow().footer.set_text("Previous version restored.");
                        notify(
                            &model,
                            Notice::Success,
                            "Rollback finished: previous version restored",
                        );
                    }
                    Err(e) => {
                        model
                            .borrow()
                            .footer
                            .set_text(&format!("Rollback failed: {e}"));
                        notify(&model, Notice::Error, &format!("Rollback failed: {e}"));
                    }
                }
                update_control_state(&model);
            });
        }
        {
            let mut m = model.borrow_mut();
            m.list.append(&row);
            m.entries.push(AppEntry {
                path,
                status,
                source_button,
                backup_button,
                normalize_button,
                update_button,
                release: None,
                availability: None,
            });
        }
    }
    model.borrow().footer.set_text(&format!(
        "Folder: {} · Configuration: {}",
        root.display(),
        config::config_path().display()
    ));
    update_control_state(model);
    notify(
        model,
        Notice::Success,
        &format!(
            "Scan complete: {} AppImages found",
            model.borrow().entries.len()
        ),
    );
}

fn update_control_state(model: &Rc<RefCell<Model>>) {
    let m = model.borrow();
    m.refresh.set_sensitive(!m.busy);
    m.check.set_sensitive(!m.busy && !m.entries.is_empty());
    m.update_all.set_sensitive(
        !m.busy
            && m.entries
                .iter()
                .any(|e| e.availability == Some(CheckState::Available)),
    );
    m.self_button.set_sensitive(!m.busy);
    m.self_button
        .set_tooltip_text(Some(if m.self_release.is_some() {
            "Install zSoft Manager update (restart required)"
        } else {
            "Check zSoft Manager updates"
        }));
    let sources = config::load_sources().unwrap_or_default();
    for e in &m.entries {
        let stable = configured_stable_name(&m.root, &e.path, &sources);
        let target = e.path.with_file_name(&stable);
        e.normalize_button
            .set_sensitive(!m.busy && e.path != target && !target.exists());
        e.normalize_button.set_tooltip_text(Some(&format!(
            "Rename to {stable} without downloading again"
        )));
        e.source_button.set_sensitive(!m.busy);
        e.backup_button
            .set_sensitive(!m.busy && update::backup_for(&e.path).is_file());
        let can_update = matches!(
            e.availability,
            Some(CheckState::Available | CheckState::Uncertain)
        ) && e.release.is_some();
        e.update_button.set_sensitive(!m.busy && can_update);
        set_button_text(
            &e.update_button,
            if e.availability == Some(CheckState::Uncertain) {
                "Install latest"
            } else {
                "Install update"
            },
        );
    }
}

fn configured_stable_name(
    root: &std::path::Path,
    path: &std::path::Path,
    sources: &config::Sources,
) -> String {
    let fallback = naming::suggested_stable_filename(path);
    let Some(dir) = scan::relative_dir(root, path) else {
        return fallback;
    };
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return fallback;
    };
    config::resolved_source(sources, &dir, name)
        .and_then(|s| s.stable_filename)
        .unwrap_or(fallback)
}

// One embedded sheet is reused by Source and Stable name. No secondary top-level
// window is created; the main panel is insensitive until the sheet is closed.
fn open_modal(model: &Rc<RefCell<Model>>, card: &gtk::Box) {
    let m = model.borrow();
    m.modal_layer.set_center_widget(Some(card));
    m.main_panel.set_sensitive(false);
    m.modal_layer.set_visible(true);
}

fn close_modal(model: &Rc<RefCell<Model>>) {
    let m = model.borrow();
    m.modal_layer.set_visible(false);
    m.modal_layer.set_center_widget(None::<&gtk::Widget>);
    m.main_panel.set_sensitive(true);
}

fn modal_card(title: &str) -> (gtk::Box, gtk::Box, gtk::Box, gtk::Button) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_size_request(520, -1);
    card.add_css_class("modal-card");

    let heading_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    heading_row.add_css_class("modal-heading-row");
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("modal-title");
    heading_row.append(&heading);
    let close = icon_only_button("window-close-symbolic", "Close");
    close.add_css_class("modal-close");
    heading_row.append(&close);
    card.append(&heading_row);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_min_content_height(170);
    scroller.set_max_content_height(430);
    scroller.set_propagate_natural_height(true);
    let form = gtk::Box::new(gtk::Orientation::Vertical, 16);
    form.add_css_class("modal-form");
    scroller.set_child(Some(&form));
    card.append(&scroller);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    actions.set_halign(gtk::Align::End);
    actions.add_css_class("modal-actions");
    card.append(&actions);
    (card, form, actions, close)
}

fn validate_source_input(source: &Source, installed_name: &str) -> Result<(), &'static str> {
    match source.provider {
        SourceProvider::Github if !config::valid_repository(&source.repository) => {
            return Err("Enter a valid GitHub owner/repository.");
        }
        SourceProvider::KdeKdenlive if !config::is_kdenlive_filename(installed_name) => {
            return Err("KDE Kdenlive source is available only for Kdenlive AppImages.");
        }
        _ => {}
    }
    if source.installed_pattern.is_empty() || source.asset_pattern.is_empty() {
        return Err("Filename patterns cannot be empty.");
    }
    if source.provider == SourceProvider::KdeKdenlive
        && source.asset_pattern != "kdenlive-*-x86_64.AppImage"
    {
        return Err("KDE downloads support only the official Kdenlive x86_64 AppImage.");
    }
    if !source
        .stable_filename
        .as_deref()
        .is_some_and(naming::valid_stable_filename)
    {
        return Err("Enter a valid stable filename ending in .AppImage.");
    }
    if !(config::wildmatch(&source.installed_pattern, installed_name)
        || source.stable_filename.as_deref() == Some(installed_name))
    {
        return Err("The installed pattern must match this AppImage's filename.");
    }
    Ok(())
}

fn confirm_normalization(model: &Rc<RefCell<Model>>, path: &PathBuf) {
    if model.borrow().busy || model.borrow().modal_layer.is_visible() {
        return;
    }
    let sources = config::load_sources().unwrap_or_default();
    let stable = configured_stable_name(&model.borrow().root, path, &sources);
    if !naming::valid_stable_filename(&stable) {
        model
            .borrow()
            .footer
            .set_text("Invalid stable filename. Check the Source settings.");
        return;
    }
    if path.file_name().and_then(|n| n.to_str()) == Some(stable.as_str()) {
        return;
    }

    let (card, form, actions, close) = modal_card("Set a stable filename");
    let message = gtk::Label::new(Some(
        "Rename this AppImage without downloading it. Existing launchers will keep working through a compatibility symlink."
    ));
    message.set_wrap(true);
    message.set_xalign(0.0);
    message.add_css_class("modal-description");
    form.append(&message);

    let filename_entry = field(&form, "Stable AppImage filename", &stable);
    let error = gtk::Label::new(None);
    error.set_xalign(0.0);
    error.set_wrap(true);
    error.add_css_class("error-text");
    form.append(&error);

    let cancel = button_with_icon("Cancel", "window-close-symbolic", "flat-button");
    cancel.add_css_class("modal-action");
    let rename = button_with_icon("Rename", "edit-rename-symbolic", "primary-button");
    rename.add_css_class("modal-action");
    actions.append(&cancel);
    actions.append(&rename);

    for dismiss in [close, cancel] {
        let m = model.clone();
        dismiss.connect_clicked(move |_| close_modal(&m));
    }
    {
        let m = model.clone();
        let path = path.clone();
        rename.connect_clicked(move |_| {
            let chosen = filename_entry.text().trim().to_owned();
            if !naming::valid_stable_filename(&chosen) {
                error.set_text(
                    "Use a .AppImage filename without slashes or unsupported characters.",
                );
                return;
            }
            match update::normalize(&path, &chosen) {
                Ok(message) => {
                    close_modal(&m);
                    rescan(&m);
                    m.borrow().footer.set_text(&message);
                    notify(&m, Notice::Success, "Stable filename saved successfully");
                }
                Err(reason) => {
                    error.set_text(&format!("Rename failed: {reason}"));
                    notify(&m, Notice::Error, &format!("Rename failed: {reason}"));
                }
            }
        });
    }
    open_modal(model, &card);
}

fn configure_source(model: &Rc<RefCell<Model>>, path: &PathBuf) {
    if model.borrow().busy || model.borrow().modal_layer.is_visible() {
        return;
    }
    let root = model.borrow().root.clone();
    let dir = match scan::relative_dir(&root, path) {
        Some(d) if !d.is_empty() => d,
        _ => return,
    };
    let name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(""))
        .to_string_lossy()
        .into_owned();
    let existing = config::load_sources()
        .ok()
        .and_then(|sources| config::resolved_source(&sources, &dir, &name));

    let (card, form, actions, close) = modal_card("Configure update source");
    let heading = gtk::Label::new(Some(&format!("Folder: {dir}")));
    heading.set_xalign(0.0);
    heading.add_css_class("modal-folder");
    form.append(&heading);

    let provider_group = gtk::Box::new(gtk::Orientation::Vertical, 6);
    provider_group.add_css_class("form-field");
    let provider_label = gtk::Label::new(Some("Source provider"));
    provider_label.set_xalign(0.0);
    provider_label.add_css_class("form-label");
    provider_group.append(&provider_label);
    let provider = gtk::ComboBoxText::new();
    provider.append(Some("github"), "GitHub Releases");
    let is_kdenlive = config::is_kdenlive_filename(&name);
    if is_kdenlive {
        provider.append(Some("kde_kdenlive"), "KDE official download server");
    }
    provider.set_active_id(Some(
        if existing
            .as_ref()
            .is_some_and(|s| s.provider == SourceProvider::KdeKdenlive)
            && is_kdenlive
        {
            "kde_kdenlive"
        } else {
            "github"
        },
    ));
    provider.add_css_class("form-input");
    provider_group.append(&provider);
    form.append(&provider_group);

    let repository = field(
        &form,
        "GitHub repository (owner/repo)",
        existing
            .as_ref()
            .map(|s| s.repository.as_str())
            .unwrap_or(""),
    );
    repository.set_placeholder_text(Some("owner/repository"));
    let kde_description = gtk::Label::new(Some(
        "Kdenlive is discovered from https://download.kde.org/stable/kdenlive/ — no repository required."));
    kde_description.set_wrap(true);
    kde_description.set_xalign(0.0);
    kde_description.set_selectable(true);
    kde_description.add_css_class("modal-description");
    form.append(&kde_description);

    let installed = field(
        &form,
        "Installed filename pattern",
        existing
            .as_ref()
            .map(|s| s.installed_pattern.as_str())
            .unwrap_or(if is_kdenlive {
                "kdenlive*.AppImage"
            } else {
                "*.AppImage"
            }),
    );
    let asset = field(
        &form,
        "Release asset pattern (e.g. *x86_64*.AppImage)",
        existing
            .as_ref()
            .map(|s| s.asset_pattern.as_str())
            .unwrap_or(if is_kdenlive {
                "kdenlive-*-x86_64.AppImage"
            } else {
                "*x86_64*.AppImage"
            }),
    );
    let suggested = if is_kdenlive {
        "kdenlive.AppImage".to_owned()
    } else {
        naming::suggested_stable_filename(path)
    };
    let stable = field(
        &form,
        "Stable filename (no version)",
        existing
            .as_ref()
            .and_then(|s| s.stable_filename.as_deref())
            .unwrap_or(&suggested),
    );

    let update_provider_view = {
        let repository = repository.clone();
        let kde_description = kde_description.clone();
        let asset = asset.clone();
        move |is_kde: bool| {
            if let Some(group) = repository.parent() {
                group.set_visible(!is_kde);
            }
            kde_description.set_visible(is_kde);
            asset.set_editable(!is_kde);
            if is_kde {
                asset.set_text("kdenlive-*-x86_64.AppImage");
            }
        }
    };
    update_provider_view(provider.active_id().as_deref() == Some("kde_kdenlive"));
    provider.connect_changed(move |picker| {
        update_provider_view(picker.active_id().as_deref() == Some("kde_kdenlive"));
    });

    let error = gtk::Label::new(None);
    error.set_xalign(0.0);
    error.set_wrap(true);
    error.add_css_class("error-text");
    form.append(&error);

    let cancel = button_with_icon("Cancel", "window-close-symbolic", "flat-button");
    cancel.add_css_class("modal-action");
    let save = button_with_icon("Save", "document-save-symbolic", "primary-button");
    save.add_css_class("modal-action");
    actions.append(&cancel);
    actions.append(&save);

    for dismiss in [close, cancel] {
        let m = model.clone();
        dismiss.connect_clicked(move |_| close_modal(&m));
    }
    let focus_widget = if provider.active_id().as_deref() == Some("kde_kdenlive") {
        None
    } else {
        Some(repository.clone())
    };
    {
        let m = model.clone();
        let path = path.clone();
        let provider_for_save = provider.clone();
        save.connect_clicked(move |_| {
            let selected_provider =
                if provider_for_save.active_id().as_deref() == Some("kde_kdenlive") {
                    SourceProvider::KdeKdenlive
                } else {
                    SourceProvider::Github
                };
            let source = Source {
                directory: dir.clone(),
                provider: selected_provider,
                repository: if selected_provider == SourceProvider::Github {
                    repository.text().trim().to_owned()
                } else {
                    String::new()
                },
                installed_pattern: installed.text().trim().to_owned(),
                asset_pattern: asset.text().trim().to_owned(),
                stable_filename: Some(stable.text().trim().to_owned()),
            };
            if let Err(message) = validate_source_input(&source, &name) {
                error.set_text(message);
                notify(&m, Notice::Error, message);
                return;
            }
            let mut sources = match config::load_sources() {
                Ok(s) => s,
                Err(reason) => {
                    error.set_text(&reason);
                    notify(
                        &m,
                        Notice::Error,
                        &format!("Source could not be loaded: {reason}"),
                    );
                    return;
                }
            };
            sources.sources.retain(|s| {
                !(s.directory == dir
                    && (config::wildmatch(&s.installed_pattern, &name)
                        || s.stable_filename.as_deref() == Some(name.as_str())))
            });
            sources.sources.push(source);
            if let Err(reason) = config::save_sources(&sources) {
                error.set_text(&format!("Could not save: {reason}"));
                notify(
                    &m,
                    Notice::Error,
                    &format!("Source could not be saved: {reason}"),
                );
                return;
            }
            if let Some(entry) = m.borrow_mut().entries.iter_mut().find(|e| e.path == path) {
                entry.release = None;
                entry.availability = None;
                entry.status.set_text("Source saved · Click Check updates");
            }
            m.borrow().footer.set_text(
                "Update source saved. Click Check updates to retrieve the latest release.",
            );
            update_control_state(&m);
            close_modal(&m);
            notify(&m, Notice::Success, "Changes saved successfully");
        });
    }
    open_modal(model, &card);
    if let Some(widget) = focus_widget {
        widget.grab_focus();
    } else {
        provider.grab_focus();
    }
}

fn field(box_: &gtk::Box, name: &str, initial: &str) -> gtk::Entry {
    let group = gtk::Box::new(gtk::Orientation::Vertical, 6);
    group.add_css_class("form-field");
    let label = gtk::Label::new(Some(name));
    label.set_xalign(0.0);
    label.add_css_class("form-label");
    group.append(&label);
    let entry = gtk::Entry::new();
    entry.set_text(initial);
    entry.add_css_class("form-input");
    entry.set_hexpand(true);
    group.append(&entry);
    box_.append(&group);
    entry
}

fn check_updates(model: &Rc<RefCell<Model>>) {
    if model.borrow().busy {
        return;
    }
    let sources = match config::load_sources() {
        Ok(s) => s,
        Err(e) => {
            model
                .borrow()
                .footer
                .set_text(&format!("Config error: {e}"));
            return;
        }
    };
    let state = config::load_installed().unwrap_or_default();
    let jobs = {
        let m = model.borrow();
        let mut jobs = Vec::new();
        for e in &m.entries {
            let file = e
                .path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new(""))
                .to_string_lossy();
            let source = scan::relative_dir(&m.root, &e.path)
                .and_then(|d| config::resolved_source(&sources, &d, &file));
            let saved = state
                .tags
                .get(&e.path.to_string_lossy().into_owned())
                .cloned();
            jobs.push((e.path.clone(), source, saved));
        }
        jobs
    };
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.footer
            .set_text("Checking configured update sources on demand…");
        for e in &mut m.entries {
            e.release = None;
            e.availability = None;
            e.status.set_text("Checking…");
        }
    }
    update_control_state(model);
    notify(model, Notice::Info, "Checking application updates…");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for (path, source, saved) in jobs {
            let result = match source {
                Some(source) => update::check(&source, &path, saved.as_deref()),
                None => Err("Update source not configured; click Source".into()),
            };
            if tx.send(Event::Checked(path, result)).is_err() {
                break;
            }
        }
        let _ = tx.send(Event::Done {
            refresh_list: false,
        });
    });
    receive_events(model.clone(), rx);
}

fn launch_update(model: &Rc<RefCell<Model>>, jobs: Vec<(PathBuf, ReleaseInfo)>) {
    if jobs.is_empty() || model.borrow().busy {
        return;
    }
    {
        let mut m = model.borrow_mut();
        m.busy = true;
        m.footer.set_text(&format!(
            "Downloading and installing {} AppImages sequentially…",
            jobs.len()
        ));
        for (path, _) in &jobs {
            if let Some(e) = m.entries.iter_mut().find(|e| &e.path == path) {
                e.status.set_text("Downloading / verifying…");
            }
        }
    }
    update_control_state(model);
    notify(model, Notice::Info, "Installing updates. Please wait…");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for (path, info) in jobs {
            let result = update::install(&path, &info);
            if tx.send(Event::Updated(path, result)).is_err() {
                break;
            }
        }
        let _ = tx.send(Event::Done { refresh_list: true });
    });
    receive_events(model.clone(), rx);
}

fn receive_events(model: Rc<RefCell<Model>>, rx: mpsc::Receiver<Event>) {
    // Timer is active only during a user-triggered network or download operation.
    // There is no idle polling and no background daemon.
    let mut update_errors: Vec<String> = Vec::new();
    let mut update_successes = 0usize;
    let mut check_available = 0usize;
    let mut check_failed = 0usize;
    let mut self_result: Option<Result<String, String>> = None;
    glib::timeout_add_local(Duration::from_millis(180), move || {
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Checked(path, result) => {
                    let mut m = model.borrow_mut();
                    if let Some(e) = m.entries.iter_mut().find(|e| e.path == path) {
                        match result {
                            Ok((info, state)) => {
                                if state == CheckState::Available {
                                    check_available += 1;
                                }
                                let digest = if info.sha256.is_some() {
                                    "SHA256 available"
                                } else {
                                    "GitHub digest unavailable"
                                };
                                let message = match state {
                                    CheckState::Available => format!(
                                        "Update available: {} · {} · {digest}",
                                        info.tag, info.asset_name
                                    ),
                                    CheckState::UpToDate => format!("Up to date: {}", info.tag),
                                    CheckState::Uncertain => format!(
                                        "Installed version unknown · Latest {} · {digest}",
                                        info.tag
                                    ),
                                };
                                e.status.set_text(&message);
                                e.status.set_tooltip_text(Some(&info.asset_name));
                                e.release = Some(info);
                                e.availability = Some(state);
                            }
                            Err(reason) => {
                                check_failed += 1;
                                e.status.set_text(&reason);
                            }
                        }
                    }
                }
                Event::Updated(path, result) => {
                    if let Err(reason) = &result {
                        let name = path
                            .file_name()
                            .unwrap_or_else(|| std::ffi::OsStr::new(""))
                            .to_string_lossy();
                        update_errors.push(format!("{name}: {reason}"));
                    } else {
                        update_successes += 1;
                    }
                    let mut m = model.borrow_mut();
                    if let Some(e) = m.entries.iter_mut().find(|e| e.path == path) {
                        match result {
                            Ok(msg) => {
                                e.status.set_text(&msg);
                                e.release = None;
                                e.availability = None;
                            }
                            Err(reason) => e.status.set_text(&format!("Update failed: {reason}")),
                        }
                    }
                }
                Event::SelfChecked(result) => match result {
                    Ok((info, CheckState::Available)) => {
                        let tag = info.tag.clone();
                        model.borrow_mut().self_release = Some(info);
                        notify(&model, Notice::Success, &format!("zSoft Manager {tag} available. Click the Update icon again to install."));
                    }
                    Ok((info, CheckState::UpToDate)) => {
                        notify(
                            &model,
                            Notice::Success,
                            &format!("zSoft Manager is up to date ({})", info.tag),
                        );
                    }
                    Ok((_, CheckState::Uncertain)) => {
                        notify(
                            &model,
                            Notice::Error,
                            "The release version could not be compared safely",
                        );
                    }
                    Err(error) => {
                        notify(
                            &model,
                            Notice::Error,
                            &format!("Self-update check failed: {error}"),
                        );
                    }
                },
                Event::SelfInstalled(result) => {
                    self_result = Some(result);
                    model.borrow_mut().self_release = None;
                }
                Event::Done { refresh_list } => {
                    model.borrow_mut().busy = false;
                    if refresh_list && update_successes > 0 {
                        rescan(&model);
                    }
                    let message = if let Some(result) = self_result.as_ref() {
                        if result.is_ok() {
                            "Self-update installed. Please restart zSoft Manager.".to_owned()
                        } else {
                            "Self-update failed. The previous AppImage was preserved.".to_owned()
                        }
                    } else if !refresh_list {
                        "Check complete. You can update or set a stable filename.".to_owned()
                    } else if update_errors.is_empty() {
                        format!("{update_successes} updates installed. List refreshed; check again for current status.")
                    } else {
                        format!(
                            "{update_successes} successful, {} failed: {}",
                            update_errors.len(),
                            update_errors.join(" | ")
                        )
                    };
                    model.borrow().footer.set_text(&message);
                    if let Some(result) = self_result.take() {
                        match result {
                            Ok(_) => notify(
                                &model,
                                Notice::Success,
                                "Update finished. Restart zSoft Manager to use the new version.",
                            ),
                            Err(error) => notify(
                                &model,
                                Notice::Error,
                                &format!("Self-update failed: {error}"),
                            ),
                        }
                    } else if refresh_list {
                        let text = format!(
                            "Update finished: {update_successes} successful, {} failed",
                            update_errors.len()
                        );
                        notify(
                            &model,
                            if update_errors.is_empty() {
                                Notice::Success
                            } else {
                                Notice::Error
                            },
                            &text,
                        );
                    } else if check_available > 0 || check_failed > 0 {
                        notify(&model, Notice::Info, &format!("Check finished: {check_available} updates available, {check_failed} sources unavailable"));
                    }
                    update_control_state(&model);
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    #[test]
    fn stack_pages_have_unique_names() {
        let ids = [
            Page::Applications.id(),
            Page::Node.id(),
            Page::Python.id(),
            Page::About.id(),
            Page::Changelog.id(),
        ];
        assert_eq!(
            ids,
            ["applications", "node", "python", "about", "changelog"]
        );
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[2], ids[3]);
        assert_ne!(ids[3], ids[4]);
    }

    #[test]
    fn main_actions_use_english_labels() {
        assert_eq!(
            MAIN_ACTION_LABELS,
            ["Rescan", "Check updates", "Update all"]
        );
        assert!(MAIN_ACTION_LABELS.iter().all(|label| label.is_ascii()));
    }

    fn source_example() -> Source {
        Source {
            directory: "BootstrapStudio".into(),
            repository: "owner/project".into(),
            installed_pattern: "*.AppImage".into(),
            asset_pattern: "*x86_64*.AppImage".into(),
            provider: SourceProvider::Github,
            stable_filename: Some("Bootstrap Studio.AppImage".into()),
        }
    }

    #[test]
    fn kde_kdenlive_source_does_not_require_github_repository() {
        let mut source = source_example();
        source.directory = "Kdenlive".into();
        source.provider = SourceProvider::KdeKdenlive;
        source.repository.clear();
        source.asset_pattern = "kdenlive-*-x86_64.AppImage".into();
        source.stable_filename = Some("kdenlive.AppImage".into());
        assert!(validate_source_input(&source, "kdenlive-26.08.0-x86_64.AppImage").is_ok());
        assert!(validate_source_input(&source, "drawio.AppImage").is_err());
    }
    #[test]
    fn valid_source_is_accepted() {
        assert!(validate_source_input(&source_example(), "bootstrap-studio-1.0.AppImage").is_ok());
    }

    #[test]
    fn invalid_repository_is_rejected() {
        let mut source = source_example();
        source.repository = "https://invalid.example/path".into();
        assert!(validate_source_input(&source, "old.AppImage").is_err());
    }

    #[test]
    fn invalid_stable_name_is_rejected() {
        let mut source = source_example();
        source.stable_filename = Some("../escape.AppImage".into());
        assert!(validate_source_input(&source, "old.AppImage").is_err());
    }

    #[test]
    fn source_requires_matching_filename() {
        let mut source = source_example();
        source.installed_pattern = "other-*.AppImage".into();
        assert!(validate_source_input(&source, "old.AppImage").is_err());
        assert!(validate_source_input(&source, "Bootstrap Studio.AppImage").is_ok());
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;

    #[test]
    fn all_header_navigation_icons_use_symbolic_gtk_icons() {
        assert_eq!(NAV_SYMBOLIC_ICONS.len(), 6);
        assert!(NAV_SYMBOLIC_ICONS
            .iter()
            .all(|name| name.ends_with("-symbolic")));
        let mut unique = NAV_SYMBOLIC_ICONS.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), NAV_SYMBOLIC_ICONS.len());
    }

    #[test]
    fn header_icon_dimensions_and_toast_margin_are_compact() {
        assert_eq!(NAV_ICON_SIZE, 20);
        assert_eq!(NAV_BUTTON_SIZE, 44);
        assert!(TOAST_BOTTOM_MARGIN > NAV_BUTTON_SIZE);
    }
}
