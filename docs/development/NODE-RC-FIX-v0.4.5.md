# zSoft Manager v0.4.5 — E0382 Rc ownership fix

The Node.js cleanup UI patch introduced an `Rc` shadowing issue in `src/node_ui.rs`.
The **Set default** callback captured a shadowed `model` by `move` and
made it unavailable to the next **Remove old** callback.

Fix: capture a new `switch_model = Rc::clone(model)` for the Set default button;
retain the outer `model` for the Remove old button.

No application version bump, no configuration changes, and no Node.js removal.

To apply from project root:

    unzip -o ~/Downloads/zSoft-Manager-v0.4.5-NODE-RC-FIX-PATCH.zip -d .
    cargo fmt
    bash scripts/audit-v045.sh

Build of Rust/GTK4 must be verified on the user's Ubuntu system.
