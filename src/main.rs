mod config;
mod info;
mod naming;
mod node;
mod node_ui;
mod python_env;
mod python_ui;
mod scan;
mod self_update;
mod ui;
mod update;

fn main() -> gtk::glib::ExitCode {
    ui::run()
}
