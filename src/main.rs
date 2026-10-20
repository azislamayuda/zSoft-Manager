mod config;
mod info;
mod naming;
mod scan;
mod self_update;
mod ui;
mod update;

fn main() -> gtk::glib::ExitCode {
    ui::run()
}
