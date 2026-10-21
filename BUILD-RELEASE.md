# zSoft Manager v0.4.2.1 — Ubuntu build and release

Cargo.toml uses `version = "0.4.2+1"` (valid SemVer); release packaging and GitHub tags use `v0.4.2.1`. The build scripts and workflow translate the `+1` metadata to `.1` in public filenames.


The downloadable source bundle contains the Rust project, but it is **not** a precompiled AppImage.
Build the binary on x86_64 Ubuntu (GTK4 development headers and the Rust stable toolchain are required).

## 1. Copy update into the existing GitHub Desktop clone

Extract `zSoft-Manager-v0.4.2.1-PATCH.zip` into:

`/home/lamayuda/ZDATA/ME/GITHUB/zSoft-Manager`

Keep your existing `.git` folder. Do not copy `.git` from anywhere else.

## 2. Test and package

In the VS Code terminal:

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
sudo apt install -y build-essential pkg-config libgtk-4-dev curl git
bash scripts/test-and-build.sh
```

If it succeeds, expect:

- `dist/zSoft-Manager-v0.4.2.1-x86_64.AppImage`
- `dist/zSoft-Manager-v0.4.2.1-x86_64.AppImage.sha256`

Test the resulting AppImage directly before publishing:

```bash
./dist/zSoft-Manager-v0.4.2.1-x86_64.AppImage
```

Try **AppImage**, **Node.js** and **Python** pages. Check that Active Node shows an LTS/Current/EOL badge; verify installed Node versions and Default Node. For a safe switcher test, choose an already-installed Node version, click **Set default**, and verify `nvm alias default` in a newly started terminal. Check that already-open terminals do not change. The global npm list should show installed packages without network access; online queries should only start when clicking **Check updates**. Inspect target versions before installing anything.

## 3. Commit and push — GitHub Desktop (no Git CLI)

Open `zSoft-Manager`, inspect changes, **Commit to main** with summary:

`feat: add Node lifecycle, version switcher and npm globals v0.4.2.1`

Then **Push origin**.

## 4. Publish the AppImage

Open the repository at https://github.com/azislamayuda/zSoft-Manager . Create a release with tag `v0.4.2.1`, title `zSoft Manager v0.4.2.1 - Navigation UI Polish`, and upload the AppImage plus `.sha256` file.

**CI conflict:** The repository already contains a tag-triggered AppImage GitHub Actions workflow. If you want to upload binaries manually, disable the workflow from GitHub Actions before publishing the release; alternatively, keep it enabled and let the workflow publish the assets. Do not use both processes to publish different binaries under the same release.

## Notes

- Self-update will work only after `v0.4.2.1` is published and a supported AppImage has been installed under a subfolder of `~/Applications`.
- Node updates only manage the user's NVM installation. No automatic removal of old Node versions.
- Titlebar appearance depends partly on Ubuntu's GTK theme and window manager.
- The AppImage must be tested on supported Ubuntu versions; merely finishing Cargo compilation is not sufficient to guarantee portability.

### Python environment smoke tests (v0.4.2.1)

- Activate a valid `~/myenv` venv and verify Python/pip information.
- Check updates manually; verify only top-level outdated packages appear.
- Test selecting a package and updating it in a disposable virtual environment before using your daily environment.
- Confirm pip check status is visible; deactivate and verify update controls are disabled.
- Confirm no terminal or system Python interpreter has been altered.

### AppImage source notes

- Bootstrap Studio: use its own license-aware updater; do not configure a guessed GitHub source.
- GitHub Copilot desktop: `github/app`, release asset `GitHub-Copilot-linux-x64.AppImage` (user-specific AppImage directory/pattern must match).


### v0.4.3 memory check (optional)

Run the existing AppImage or release binary, then in another terminal:

```bash
ps -C zsoft-manager -o pid,rss,etime,args
```

`RSS` is in KiB. Compare an idle AppImage-only session with v0.4.2.1 under the same display/backend. Memory will rise after first opening the Node.js or Python page because their widgets are then allocated.
