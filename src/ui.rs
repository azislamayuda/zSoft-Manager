use crate::{
    apt_ui,
    config::{self, Source, SourceProvider},
    dashboard_ui, flatpak_ui, info, naming, node_ui, python_ui, scan, self_update,
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
    LocalVersionConfirmed(PathBuf, Result<CheckState, String>),
    SelfChecked(Result<self_update::SelfUpdateStatus, String>),
    SelfInstalled(Result<String, String>),
    Done { refresh_list: bool },
}

// GtkStack keeps navigation inside the main window. Dashboard inventory
// uses short-lived local workers; About and Changelog are offline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Dashboard,
    Applications,
    Flatpak,
    Apt,
    Node,
    Python,
    About,
}

impl Page {
    fn id(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Applications => "applications",
            Self::Flatpak => "flatpak",
            Self::Apt => "apt",
            Self::Node => "node",
            Self::Python => "python",
            Self::About => "about",
        }
    }
}

const MAIN_ACTION_LABELS: [&str; 3] = ["Rescan", "Check updates", "Update all"];

// All navigation assets are GTK symbolic icons. Using theme icons instead of
// GtkImage::from_paintable keeps Node and Python at the same pixel size as the
// About and Changelog icons, regardless of the original PNG dimensions.
const NAV_SYMBOLIC_ICONS: [&str; 8] = [
    "view-grid-symbolic",                 // Dashboard
    "application-x-executable-symbolic",  // AppImage
    "application-x-addon-symbolic",       // Flatpak: add-on / puzzle
    "package-x-generic-symbolic",         // APT: installed software packages
    "utilities-terminal-symbolic",        // Node.js
    "applications-development-symbolic",  // Python
    "help-about-symbolic",                // About + Changelog
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

fn set_button_content(button: &gtk::Button, label: &str, icon_name: &str) {
    if let Some(content) = button
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    {
        if let Some(icon) = content
            .first_child()
            .and_then(|child| child.downcast::<gtk::Image>().ok())
        {
            icon.set_icon_name(Some(icon_name));
        }
        if let Some(text) = content
            .last_child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
        {
            text.set_text(label);
        }
    }
}

/// A matching installed version is a status, not an invitation to install again.
/// Keep the primary button for updates and first-time version confirmation only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppAction {
    CheckUpdates,
    InstallUpdate,
    SetLocalVersion,
    UpToDate,
}

fn app_action(state: Option<CheckState>, has_release: bool) -> AppAction {
    if !has_release {
        return AppAction::CheckUpdates;
    }
    match state {
        Some(CheckState::Available) => AppAction::InstallUpdate,
        Some(CheckState::Uncertain) => AppAction::SetLocalVersion,
        Some(CheckState::UpToDate | CheckState::UserConfirmed) => AppAction::UpToDate,
        None => AppAction::CheckUpdates,
    }
}

fn action_ui(action: AppAction) -> (&'static str, &'static str, &'static str) {
    match action {
        AppAction::CheckUpdates => ("Check updates", "view-refresh-symbolic", "Run Check updates before installing"),
        AppAction::InstallUpdate => ("Install update", "software-update-available-symbolic", "Install the confirmed newer AppImage (backup available for rollback)"),
        AppAction::SetLocalVersion => ("Set local version", "document-edit-symbolic", "Confirm the version shown in the installed application's About screen; no download"),
        AppAction::UpToDate => ("Up to date", "emblem-ok-symbolic", "This AppImage is current. If manually confirmed, change its recorded version using Source."),
    }
}

fn mark_self_update_retry(model: &Rc<RefCell<Model>>, retry: bool) {
    let m = model.borrow();
    m.self_button.set_tooltip_text(Some(if retry {
        "Retry: the GitHub release assets are not ready yet"
    } else {
        "Check zSoft Manager updates"
    }));
    if let Some(image) = m
        .self_button
        .child()
        .and_then(|child| child.downcast::<gtk::Image>().ok())
    {
        image.set_icon_name(Some(if retry {
            "view-refresh-symbolic"
        } else {
            "software-update-available-symbolic"
        }));
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
    dashboard: gtk::Button,
    appimage: gtk::Button,
    flatpak: gtk::Button,
    apt: gtk::Button,
    node: gtk::Button,
    python: gtk::Button,
    about: gtk::Button,
}

fn navigate(nav: &Navigation, page: Page) {
    nav.stack.set_visible_child_name(page.id());
    for (button, destination) in [
        (&nav.dashboard, Page::Dashboard),
        (&nav.appimage, Page::Applications),
        (&nav.flatpak, Page::Flatpak),
        (&nav.apt, Page::Apt),
        (&nav.node, Page::Node),
        (&nav.python, Page::Python),
        (&nav.about, Page::About),
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
    let dashboard_nav = icon_only_button(
        NAV_SYMBOLIC_ICONS[0],
        "Dashboard / Installed software overview",
    );
    let appimage_nav = icon_only_button(NAV_SYMBOLIC_ICONS[1], "AppImage Manager");
    let flatpak_nav = icon_only_button(NAV_SYMBOLIC_ICONS[2], "Flatpak Manager");
    let apt_nav = icon_only_button(NAV_SYMBOLIC_ICONS[3], "APT Manager / Ubuntu Maintenance");
    let node_nav = icon_only_button(NAV_SYMBOLIC_ICONS[4], "Node.js / npm / NVM");
    let python_nav = icon_only_button(NAV_SYMBOLIC_ICONS[5], "Python environments / pip");
    let about_nav = icon_only_button(NAV_SYMBOLIC_ICONS[6], "About & Changelog");
    navigation.append(&dashboard_nav);
    navigation.append(&appimage_nav);
    navigation.append(&flatpak_nav);
    navigation.append(&apt_nav);
    navigation.append(&node_nav);
    navigation.append(&python_nav);
    navigation.append(&about_nav);
    let self_button = icon_only_button(NAV_SYMBOLIC_ICONS[7], "Check zSoft Manager updates");
    navigation.append(&self_button);
    hero.append(&navigation);
    root_panel.append(&hero);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(150);
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    // The landing page provides local read-only inventories in lightweight cards.
    // Workers run only for the dashboard, not for other page navigation.
    let dashboard_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_named(&dashboard_page, Some(Page::Dashboard.id()));

    // APPLICATIONS PAGE: existing AppImage management controls stay intact.
    let apps_page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    apps_page.add_css_class("page");
    let apps_header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    apps_header.add_css_class("page-header");
    let apps_heading = gtk::Label::new(Some("AppImage Manager"));
    apps_heading.add_css_class("page-heading");
    apps_heading.set_xalign(0.0);
    apps_heading.set_hexpand(true);
    apps_header.append(&apps_heading);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.add_css_class("action-bar");
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::Center);
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
    apps_header.append(&actions);
    apps_page.append(&apps_header);

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

    // Flatpak initializes only after the navigation icon is opened.
    let flatpak_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_named(&flatpak_page, Some(Page::Flatpak.id()));
    let flatpak_loaded = Rc::new(Cell::new(false));
    let apt_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_named(&apt_page, Some(Page::Apt.id()));
    let apt_loaded = Rc::new(Cell::new(false));

    // Build heavy toolchain pages only when opened. This avoids starting Node
    // subprocesses at startup and reduces GTK widget allocations on AppImage-only sessions.
    let node_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_named(&node_page, Some(Page::Node.id()));
    let python_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_named(&python_page, Some(Page::Python.id()));
    let node_loaded = Rc::new(Cell::new(false));
    let python_loaded = Rc::new(Cell::new(false));

    // Single About page: separate offline cards in one scrollable document.
    let about_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    about_page.add_css_class("page");
    let about_scroll = gtk::ScrolledWindow::new();
    about_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    about_scroll.set_vexpand(true);
    let about_sections = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let about_content = gtk::Box::new(gtk::Orientation::Vertical, 15);
    about_content.add_css_class("content-card");
    let about_title = gtk::Label::new(Some("About"));
    about_title.add_css_class("page-heading");
    about_title.set_xalign(0.0);
    about_content.append(&about_title);
    let about_description = gtk::Label::new(Some(
        "A lightweight native software manager for Ubuntu. Manage AppImages, Flatpak applications and runtimes, Ubuntu APT packages, Node.js, npm, and Python environments."
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
        "GitHub Releases · KDE Downloads · Flatpak remotes · Ubuntu APT · nvm-sh · Node.js · npm",
    );
    about_content.append(&details);
    let project_link = button_with_icon(
        "Visit GitHub @azislamayuda",
        "web-browser-symbolic",
        "soft-button",
    );
    project_link.set_halign(gtk::Align::Start);
    about_content.append(&project_link);
    about_sections.append(&about_content);

    let changelog_card = gtk::Box::new(gtk::Orientation::Vertical, 12);
    changelog_card.add_css_class("content-card");
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
    changelog_card.append(&changelog_header);
    let changelog_scroll = gtk::ScrolledWindow::new();
    changelog_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    changelog_scroll.set_height_request(380);
    changelog_scroll.add_css_class("changelog-scroll");
    let changelog_view = gtk::TextView::new();
    changelog_view.set_editable(false);
    changelog_view.set_cursor_visible(false);
    changelog_view.set_wrap_mode(gtk::WrapMode::WordChar);
    changelog_view.set_top_margin(16);
    changelog_view.set_bottom_margin(16);
    changelog_view.set_left_margin(16);
    changelog_view.set_right_margin(16);
    changelog_view.add_css_class("changelog-text");
    changelog_view.buffer().set_text(info::CHANGELOG);
    changelog_scroll.set_child(Some(&changelog_view));
    changelog_card.append(&changelog_scroll);
    about_sections.append(&changelog_card);
    about_scroll.set_child(Some(&about_sections));
    about_page.append(&about_scroll);
    stack.add_named(&about_page, Some(Page::About.id()));
    let nav = Navigation {
        stack: stack.clone(),
        dashboard: dashboard_nav.clone(),
        appimage: appimage_nav.clone(),
        flatpak: flatpak_nav.clone(),
        apt: apt_nav.clone(),
        node: node_nav.clone(),
        python: python_nav.clone(),
        about: about_nav.clone(),
    };
    // Dashboard is read-only: cards show quantities and summaries, without
    // buttons or per-application names. Navigation stays in the header.
    dashboard_page.append(&dashboard_ui::build(&window));
    navigate(&nav, Page::Dashboard);
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
    let toast_sequence = Rc::new(Cell::new(0_u64));

    for (page, button) in [
        (Page::Dashboard, dashboard_nav.clone()),
        (Page::Applications, appimage_nav.clone()),
        (Page::Flatpak, flatpak_nav.clone()),
        (Page::Apt, apt_nav.clone()),
        (Page::Node, node_nav.clone()),
        (Page::Python, python_nav.clone()),
        (Page::About, about_nav.clone()),
    ] {
        let nav = nav.clone();
        let flatpak_page = flatpak_page.clone();
        let flatpak_toast = toast.clone();
        let flatpak_label = toast_label.clone();
        let flatpak_sequence = toast_sequence.clone();
        let flatpak_loaded = flatpak_loaded.clone();
        let apt_loaded = apt_loaded.clone();
        let apt_page = apt_page.clone();
        let node_page = node_page.clone();
        let python_page = python_page.clone();
        let node_loaded = node_loaded.clone();
        let python_loaded = python_loaded.clone();
        button.connect_clicked(move |_| {
            if page == Page::Flatpak && !flatpak_loaded.replace(true) {
                flatpak_page.append(&flatpak_ui::build(page_notice(
                    &flatpak_toast,
                    &flatpak_label,
                    &flatpak_sequence,
                )));
            }
            if page == Page::Apt && !apt_loaded.replace(true) {
                apt_page.append(&apt_ui::build(page_notice(
                    &flatpak_toast,
                    &flatpak_label,
                    &flatpak_sequence,
                )));
            }
            if page == Page::Node && !node_loaded.replace(true) {
                node_page.append(&node_ui::build(page_notice(
                    &flatpak_toast,
                    &flatpak_label,
                    &flatpak_sequence,
                )));
            }
            if page == Page::Python && !python_loaded.replace(true) {
                python_page.append(&python_ui::build(page_notice(
                    &flatpak_toast,
                    &flatpak_label,
                    &flatpak_sequence,
                )));
            }
            navigate(&nav, page);
        });
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
        toast_sequence,
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

// Pages use the same toast surface and sequence guard as the AppImage page.
// A later toast must not be hidden by an earlier timeout.
fn page_notice(
    toast: &gtk::Revealer,
    label: &gtk::Label,
    sequence: &Rc<Cell<u64>>,
) -> Rc<dyn Fn(&str, bool)> {
    let popup = toast.clone();
    let text = label.clone();
    let sequence = sequence.clone();
    Rc::new(move |message, success| {
        for css in ["toast-success", "toast-info", "toast-error"] {
            popup.remove_css_class(css);
        }
        popup.add_css_class(if success {
            "toast-success"
        } else {
            "toast-error"
        });
        text.set_text(message);
        popup.set_reveal_child(true);
        let ticket = sequence.get().wrapping_add(1);
        sequence.set(ticket);
        let popup = popup.clone();
        let sequence = sequence.clone();
        glib::timeout_add_local_once(Duration::from_secs(5), move || {
            if sequence.get() == ticket {
                popup.set_reveal_child(false);
            }
        });
    })
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
    mark_self_update_retry(model, false);
    notify(model, Notice::Info, "Checking zSoft Manager updates…");
    update_control_state(model);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(Event::SelfChecked(self_update::check_release()));
        let _ = tx.send(Event::Done {
            refresh_list: false,
        });
    });
    receive_events(model.clone(), rx, false);
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
    receive_events(model.clone(), rx, false);
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
        m.count.set_text(&format!(
            "{} AppImages found · Subfolders only · ArtCraft excluded",
            files.len()
        ));
        // Preserve existing GTK rows, callbacks and labels when the paths did not change.
        // Rebuilding a large list on every Rescan creates avoidable allocations.
        if files.len() == m.entries.len()
            && files
                .iter()
                .zip(&m.entries)
                .all(|(found, entry)| found == &entry.path)
        {
            m.footer.set_text("Scan complete. AppImage list unchanged.");
            drop(m);
            update_control_state(model);
            return;
        }
        while let Some(row) = m.list.first_child() {
            m.list.remove(&row);
        }
        m.entries.clear();
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
        let update_button =
            button_with_icon("Check updates", "view-refresh-symbolic", "soft-button");
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
                let selected = model
                    .borrow()
                    .entries
                    .iter()
                    .find(|e| e.path == path_for_update)
                    .map(|e| (e.release.clone(), e.availability));
                if let Some((Some(release), availability)) = selected {
                    match app_action(availability, true) {
                        AppAction::SetLocalVersion => {
                            confirm_local_version(&model, &path_for_update)
                        }
                        AppAction::InstallUpdate => {
                            launch_update(&model, vec![(path_for_update.clone(), release)])
                        }
                        AppAction::UpToDate | AppAction::CheckUpdates => {}
                    }
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
        let action = app_action(e.availability, e.release.is_some());
        let (label, icon, tooltip) = action_ui(action);
        // Up-to-date files must never offer a reinstall on the main row,
        // including user-confirmed installations without an upstream checksum.
        e.update_button.set_sensitive(
            !m.busy
                && matches!(
                    action,
                    AppAction::InstallUpdate | AppAction::SetLocalVersion
                ),
        );
        set_button_content(&e.update_button, label, icon);
        e.update_button.set_tooltip_text(Some(tooltip));
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

/// For manually installed versionless AppImages, never assume the upstream
/// version based solely on a stable filename or file timestamp. The user may
/// record the version reported by the application's About screen. An SHA256
/// fingerprint detects later local replacements and revokes that assertion.
fn confirm_local_version(model: &Rc<RefCell<Model>>, path: &PathBuf) {
    if model.borrow().busy || model.borrow().modal_layer.is_visible() {
        return;
    }
    let (release, source, already_current) = {
        let m = model.borrow();
        let Some(entry) = m.entries.iter().find(|entry| entry.path == *path) else {
            return;
        };
        let already_current = matches!(
            entry.availability,
            Some(CheckState::UpToDate | CheckState::UserConfirmed)
        );
        let Some(release) = entry.release.clone() else {
            return;
        };
        let sources = match config::load_sources() {
            Ok(sources) => sources,
            Err(error) => {
                m.footer.set_text(&format!("Sources unavailable: {error}"));
                return;
            }
        };
        let dir = scan::relative_dir(&m.root, path).unwrap_or_default();
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(source) = config::resolved_source(&sources, &dir, filename) else {
            return;
        };
        (release, source, already_current)
    };
    if source.provider != SourceProvider::Github {
        model
            .borrow()
            .footer
            .set_text("Manual version confirmation supports GitHub sources only.");
        return;
    }
    let (card, form, actions, close) = modal_card("Confirm installed AppImage version");
    let description = gtk::Label::new(Some(
        "If the installed application's About screen reports this release version, save it here without downloading anything. This is your confirmation, not publisher SHA256 verification. Changing the local AppImage will invalidate the confirmation."
    ));
    description.set_wrap(true);
    description.set_xalign(0.0);
    description.add_css_class("modal-description");
    form.append(&description);
    let previous_tag = config::load_installed()
        .ok()
        .and_then(|installed| {
            installed
                .local_version_confirmations
                .get(path.to_string_lossy().as_ref())
                .filter(|record| record.repository == source.repository)
                .map(|record| record.tag.clone())
        })
        .unwrap_or_default();
    let installed_tag = field(
        &form,
        "Version currently installed (release tag)",
        &previous_tag,
    );
    installed_tag.set_placeholder_text(Some(
        "Enter the version shown in the installed app's About screen",
    ));
    let latest = gtk::Label::new(Some(&format!("Latest GitHub release: {}", release.tag)));
    latest.set_xalign(0.0);
    latest.add_css_class("modal-description");
    form.append(&latest);
    let error = gtk::Label::new(None);
    error.set_wrap(true);
    error.set_xalign(0.0);
    error.add_css_class("error-text");
    form.append(&error);

    let cancel = button_with_icon("Cancel", "window-close-symbolic", "flat-button");
    let reinstall = button_with_icon(
        "Reinstall latest",
        "software-update-available-symbolic",
        "subtle-button",
    );
    let save = button_with_icon("Confirm version", "emblem-ok-symbolic", "primary-button");
    actions.append(&cancel);
    if !already_current {
        actions.append(&reinstall);
    }
    actions.append(&save);
    for dismiss in [close, cancel] {
        let model = model.clone();
        dismiss.connect_clicked(move |_| close_modal(&model));
    }
    {
        let model = model.clone();
        let path = path.clone();
        let release = release.clone();
        reinstall.connect_clicked(move |_| {
            close_modal(&model);
            launch_update(&model, vec![(path.clone(), release.clone())]);
        });
    }
    {
        let model = model.clone();
        let path = path.clone();
        save.connect_clicked(move |_| {
            let tag = installed_tag.text().trim().to_owned();
            if tag.is_empty() || tag.len() > 128 || tag.chars().any(char::is_control) {
                error.set_text("Enter a valid version or release tag (up to 128 characters).");
                return;
            }
            close_modal(&model);
            let (tx, rx) = mpsc::channel();
            {
                let mut m = model.borrow_mut();
                m.busy = true;
                m.footer
                    .set_text("Recording local AppImage version without reinstalling…");
                if let Some(entry) = m.entries.iter_mut().find(|e| e.path == path) {
                    entry.status.set_text("Checking local file fingerprint…");
                }
            }
            update_control_state(&model);
            let source = source.clone();
            let release = release.clone();
            let worker_path = path.clone();
            thread::spawn(move || {
                let result =
                    update::confirm_installed_version(&worker_path, &source, &release, &tag);
                let _ = tx.send(Event::LocalVersionConfirmed(worker_path, result));
                let _ = tx.send(Event::Done {
                    refresh_list: false,
                });
            });
            receive_events(model.clone(), rx, false);
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
    // A manual version can be corrected from Source, not from the Up to date
    // row action. Only show this control for an existing GitHub confirmation.
    let has_saved_local_version = existing.as_ref().is_some_and(|source| {
        source.provider == SourceProvider::Github
            && config::load_installed().ok().is_some_and(|installed| {
                installed
                    .local_version_confirmations
                    .get(path.to_string_lossy().as_ref())
                    .is_some_and(|record| record.repository == source.repository)
            })
    });
    let has_checked_release = model
        .borrow()
        .entries
        .iter()
        .any(|entry| entry.path == *path && entry.release.is_some());

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
    if has_saved_local_version && has_checked_release {
        let edit_version = button_with_icon(
            "Edit local version",
            "document-edit-symbolic",
            "subtle-button",
        );
        edit_version.set_tooltip_text(Some(
            "Edit the previously confirmed version. Unsaved Source fields will be discarded.",
        ));
        actions.append(&edit_version);
        let m = model.clone();
        let selected_path = path.clone();
        edit_version.connect_clicked(move |_| {
            close_modal(&m);
            confirm_local_version(&m, &selected_path);
        });
    }
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
            notify(
                model,
                Notice::Error,
                &format!("AppImage update check failed: {e}"),
            );
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
        // Never hammer GitHub once its rate limit has been reached. KDE
        // checks can still proceed because they use a different provider.
        let mut github_limited = false;
        for (path, source, saved) in jobs {
            let result = match source {
                Some(source) if github_limited && source.provider == SourceProvider::Github => Err(
                    "GitHub API rate limit exceeded; check skipped until the limit resets".into(),
                ),
                Some(source) => {
                    let result = update::check(&source, &path, saved.as_deref());
                    if result
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.starts_with("GitHub API rate limit exceeded"))
                    {
                        github_limited = true;
                    }
                    result
                }
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
    receive_events(model.clone(), rx, true);
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
    receive_events(model.clone(), rx, false);
}

fn receive_events(model: Rc<RefCell<Model>>, rx: mpsc::Receiver<Event>, report_check: bool) {
    // Timer is active only during a user-triggered network or download operation.
    // There is no idle polling and no background daemon.
    let mut update_errors: Vec<String> = Vec::new();
    let mut update_successes = 0usize;
    let mut check_available = 0usize;
    let mut check_failed = 0usize;
    let mut check_current = 0usize;
    let mut check_uncertain = 0usize;
    let mut self_result: Option<Result<String, String>> = None;
    let mut pending_self_check: Option<String> = None;
    let mut local_version_recorded = false;
    glib::timeout_add_local(Duration::from_millis(180), move || {
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Checked(path, result) => {
                    let mut m = model.borrow_mut();
                    if let Some(e) = m.entries.iter_mut().find(|e| e.path == path) {
                        match result {
                            Ok((info, state)) => {
                                match state {
                                    CheckState::Available => check_available += 1,
                                    CheckState::UpToDate | CheckState::UserConfirmed => {
                                        check_current += 1
                                    }
                                    CheckState::Uncertain => check_uncertain += 1,
                                }
                                let digest = if info.sha256.is_some() {
                                    "SHA256 available"
                                } else {
                                    "GitHub digest unavailable"
                                };
                                let message = match state {
                                    CheckState::Available => format!("Update available: {} · {} · {digest}", info.tag, info.asset_name),
                                    CheckState::UpToDate => format!("Up to date: {}", info.tag),
                                    CheckState::UserConfirmed => format!("Up to date: {} · Local version user-confirmed (not publisher-verified)", info.tag),
                                    CheckState::Uncertain => format!("Local version unverified · Latest {} · {digest} · No update confirmed", info.tag),
                                };
                                e.status.set_text(&message);
                                e.status.set_tooltip_text(Some(&info.asset_name));
                                e.release = Some(info);
                                e.availability = Some(state);
                            }
                            Err(reason) => {
                                check_failed += 1;
                                e.status.set_tooltip_text(Some(&reason));
                                e.status.set_text(&reason);
                            }
                        }
                    }
                }
                Event::LocalVersionConfirmed(path, result) => {
                    let mut m = model.borrow_mut();
                    let status_message = match result {
                        Ok(state) => {
                            local_version_recorded = true;
                            if let Some(entry) = m.entries.iter_mut().find(|e| e.path == path) {
                                entry.availability = Some(state);
                                if let Some(info) = &entry.release {
                                    entry.status.set_text(&match state {
                                        CheckState::UserConfirmed => format!("Up to date: {} · Local version user-confirmed (not publisher-verified)", info.tag),
                                        CheckState::UpToDate => format!("Up to date: {}", info.tag),
                                        CheckState::Available => format!("Update available: {} · Based on confirmed installed version", info.tag),
                                        CheckState::Uncertain => "Installed version recorded · Comparison unavailable".into(),
                                    });
                                }
                            }
                            "Local version saved without downloading or modifying the AppImage."
                        }
                        Err(ref reason) => {
                            if let Some(entry) = m.entries.iter_mut().find(|e| e.path == path) {
                                entry.status.set_text(&format!(
                                    "Could not record installed version: {reason}"
                                ));
                            }
                            "Unable to confirm the local AppImage version."
                        }
                    };
                    m.footer.set_text(status_message);
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
                    Ok(self_update::SelfUpdateStatus::Ready(info, CheckState::Available)) => {
                        let tag = info.tag.clone();
                        model.borrow_mut().self_release = Some(info);
                        mark_self_update_retry(&model, false);
                        notify(&model, Notice::Success,
                                &format!("zSoft Manager {tag} is verified and ready. Click Update again to install."));
                    }
                    Ok(self_update::SelfUpdateStatus::Ready(info, CheckState::UpToDate)) => {
                        mark_self_update_retry(&model, false);
                        notify(
                            &model,
                            Notice::Success,
                            &format!("zSoft Manager is up to date ({})", info.tag),
                        );
                    }
                    Ok(self_update::SelfUpdateStatus::Ready(
                        _,
                        CheckState::UserConfirmed | CheckState::Uncertain,
                    )) => {
                        notify(
                            &model,
                            Notice::Error,
                            "Release version could not be compared safely",
                        );
                    }
                    Ok(self_update::SelfUpdateStatus::Pending { tag, reason }) => {
                        let message = format!(
                            "Release {tag} found, but not ready: {reason}. Click Update to retry."
                        );
                        model.borrow().footer.set_text(&message);
                        pending_self_check = Some(message.clone());
                        mark_self_update_retry(&model, true);
                        notify(&model, Notice::Info, &message);
                    }
                    Err(error) => {
                        mark_self_update_retry(&model, true);
                        notify(
                            &model,
                            Notice::Error,
                            &format!("Self-update check failed: {error}. Click Update to retry."),
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
                    } else if let Some(reason) = pending_self_check.take() {
                        reason
                    } else if local_version_recorded {
                        "Local installed version recorded. No AppImage was downloaded or replaced."
                            .to_owned()
                    } else if report_check {
                        crate::check_summary::appimage(
                            check_available,
                            check_current,
                            check_uncertain,
                            check_failed,
                        )
                    } else if !refresh_list {
                        "Operation complete.".to_owned()
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
                    } else if report_check {
                        let text = crate::check_summary::appimage(
                            check_available,
                            check_current,
                            check_uncertain,
                            check_failed,
                        );
                        let kind = if check_failed > 0 {
                            Notice::Error
                        } else if check_available > 0 {
                            Notice::Info
                        } else {
                            Notice::Success
                        };
                        notify(&model, kind, &text);
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
            Page::Dashboard.id(),
            Page::Applications.id(),
            Page::Flatpak.id(),
            Page::Apt.id(),
            Page::Node.id(),
            Page::Python.id(),
            Page::About.id(),
        ];
        assert_eq!(
            ids,
            [
                "dashboard",
                "applications",
                "flatpak",
                "apt",
                "node",
                "python",
                "about"
            ]
        );
        let mut unique = ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn combined_about_page_is_offline_and_without_legacy_controls() {
        let source = include_str!("ui.rs");
        let app_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!app_source.contains("Back to applications"));
        assert!(!app_source.contains("detail_row(&details, \"Network\""));
        assert!(!app_source.contains("Page::Changelog"));
        assert!(app_source.contains("about_sections.append(&changelog_card);"));
    }

    #[test]
    fn appimage_primary_actions_share_the_subheader() {
        let full_source = include_str!("ui.rs");
        let production = full_source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(full_source);
        assert!(production.contains("apps_header.append(&actions);"));
        assert!(production.contains("apps_page.append(&apps_header);"));
        assert!(production.contains("AppImage Manager"));
        assert!(!production.contains("apps_page.append(&actions);"));
    }

    #[test]
    fn flatpak_and_apt_navigation_have_distinct_theme_icons() {
        assert_eq!(NAV_SYMBOLIC_ICONS[2], "application-x-addon-symbolic");
        assert_eq!(NAV_SYMBOLIC_ICONS[3], "package-x-generic-symbolic");
    }

    #[test]
    fn main_actions_use_english_labels() {
        assert_eq!(
            MAIN_ACTION_LABELS,
            ["Rescan", "Check updates", "Update all"]
        );
        assert!(MAIN_ACTION_LABELS.iter().all(|label| label.is_ascii()));
    }

    #[test]
    fn up_to_date_is_a_disabled_status_not_a_reinstall_action() {
        for state in [CheckState::UpToDate, CheckState::UserConfirmed] {
            let action = app_action(Some(state), true);
            assert_eq!(action, AppAction::UpToDate);
            assert_eq!(action_ui(action).0, "Up to date");
            assert_eq!(action_ui(action).1, "emblem-ok-symbolic");
        }
    }

    #[test]
    fn genuine_updates_and_unknown_local_versions_keep_distinct_actions() {
        assert_eq!(
            app_action(Some(CheckState::Available), true),
            AppAction::InstallUpdate
        );
        assert_eq!(
            app_action(Some(CheckState::Uncertain), true),
            AppAction::SetLocalVersion
        );
        assert_eq!(app_action(None, false), AppAction::CheckUpdates);
        assert_eq!(
            app_action(Some(CheckState::Available), false),
            AppAction::CheckUpdates
        );
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
        assert_eq!(NAV_SYMBOLIC_ICONS.len(), 8);
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
