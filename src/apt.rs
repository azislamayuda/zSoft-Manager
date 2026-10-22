//! APT backend for Ubuntu. Read-only simulations never need privilege.
//! Only fixed apt-get subcommands are executed through the system's polkit agent.
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Upgrade,
    Autoremove,
    Autoclean,
}
impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Refresh => "Refresh package lists",
            Self::Upgrade => "Upgrade packages",
            Self::Autoremove => "Autoremove",
            Self::Autoclean => "Autoclean",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageChange {
    pub name: String,
    pub installed: String,
    pub candidate: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub action: Action,
    pub install: Vec<PackageChange>,
    pub remove: Vec<String>,
    pub summary: String,
}
#[derive(Clone, Debug)]
pub struct Overview {
    pub upgrade: Plan,
    pub autoremove: Result<Plan, String>,
    pub held: Vec<String>,
    pub reboot_required: bool,
}

fn call_readonly(program: &str, args: &[&str]) -> Result<String, String> {
    let result = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("Could not start {program}: {e}"))?;
    if !result.status.success() {
        let err = String::from_utf8_lossy(&result.stderr);
        return Err(format!("{} failed: {}", program, err.trim()));
    }
    String::from_utf8(result.stdout).map_err(|e| format!("Invalid APT output: {e}"))
}
fn parse_action(text: &str, action: Action) -> Result<Plan, String> {
    let mut install = Vec::new();
    let mut remove = Vec::new();
    let mut summary = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(body) = line.strip_prefix("Inst ") {
            let mut pieces = body.split_whitespace();
            let name = pieces.next().unwrap_or("").to_owned();
            if name.is_empty() {
                return Err("APT simulation contained an unnamed package".into());
            }
            let installed = body
                .split('[')
                .nth(1)
                .and_then(|s| s.split(']').next())
                .unwrap_or("Not installed")
                .to_owned();
            let candidate = body
                .split('(')
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("Unknown")
                .to_owned();
            install.push(PackageChange {
                name,
                installed,
                candidate,
            });
        } else if let Some(body) = line.strip_prefix("Remv ") {
            let name = body.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                return Err("APT simulation contained an unnamed removal".into());
            }
            remove.push(name.to_owned());
        } else if line.contains(" upgraded,") && line.contains(" to remove") {
            summary = line.to_owned();
        }
    }
    if summary.is_empty() {
        return Err(
            "APT simulation did not return a recognizable operation summary; no action will run"
                .into(),
        );
    }
    // Never trust a partially parsed removal plan: the count in the APT
    // summary must agree with all removal lines shown in the simulation.
    let removal_count = summary
        .split(" to remove")
        .next()
        .and_then(|before| before.split_whitespace().last())
        .and_then(|count| count.parse::<usize>().ok())
        .ok_or("Could not verify removal count in APT simulation")?;
    if removal_count != remove.len() {
        return Err(
            "APT simulation removal count does not match the detailed list; blocked".into(),
        );
    }
    install.sort_by(|a, b| a.name.cmp(&b.name));
    remove.sort();
    Ok(Plan {
        action,
        install,
        remove,
        summary,
    })
}
pub fn preview(action: Action) -> Result<Plan, String> {
    let command = match action {
        Action::Upgrade => "upgrade",
        Action::Autoremove => "autoremove",
        _ => return Err("This action has no package simulation".into()),
    };
    // Simulation deliberately runs without root; no package state is modified.
    let output = call_readonly(
        "/usr/bin/apt-get",
        &[
            "-s",
            "-o",
            "APT::Get::Show-User-Simulation-Note=false",
            command,
        ],
    )?;
    let plan = parse_action(&output, action)?;
    validate_plan(&plan)?;
    Ok(plan)
}
fn critical(name: &str) -> bool {
    let base = name.split(':').next().unwrap_or(name).to_ascii_lowercase();
    [
        "linux-image",
        "linux-headers",
        "linux-modules",
        "linux-generic",
        "linux-firmware",
        "ubuntu-",
        "grub",
        "shim",
        "systemd",
        "libc6",
        "dpkg",
        "apt",
        "network-manager",
        "gdm3",
        "nvidia-",
        "firmware-",
    ]
    .iter()
    .any(|prefix| base == *prefix || base.starts_with(prefix))
}
pub fn validate_plan(plan: &Plan) -> Result<(), String> {
    match plan.action {
        Action::Upgrade if !plan.remove.is_empty()=>Err("APT upgrade preview proposes removals; blocked. Use Ubuntu's system updater for complex changes.".into()),
        Action::Autoremove if !plan.install.is_empty()=>Err("Unexpected install during autoremove; blocked.".into()),
        Action::Autoremove if plan.remove.iter().any(|p|critical(p))=>Err(format!("Autoremove includes protected system packages ({}); operation blocked.",plan.remove.iter().filter(|p|critical(p)).cloned().collect::<Vec<_>>().join(", "))),
        _=>Ok(())
    }
}
pub fn overview() -> Result<Overview, String> {
    let upgrade = preview(Action::Upgrade)?;
    let autoremove = preview(Action::Autoremove);
    let held = call_readonly("/usr/bin/apt-mark", &["showhold"])?
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    Ok(Overview {
        upgrade,
        autoremove,
        held,
        reboot_required: Path::new("/var/run/reboot-required").exists(),
    })
}
fn privileged_args(action: Action) -> Vec<&'static str> {
    let mut a = vec![
        "/usr/bin/apt-get",
        "-o",
        "Dpkg::Use-Pty=0",
        "-o",
        "Dpkg::Options::=--force-confdef",
        "-o",
        "Dpkg::Options::=--force-confold",
    ];
    match action {
        Action::Refresh => a.extend(["update"]),
        Action::Upgrade => a.extend(["--assume-yes", "--no-remove", "upgrade"]),
        Action::Autoremove => a.extend(["--assume-yes", "autoremove"]),
        Action::Autoclean => a.extend(["autoclean"]),
    }
    a
}
#[derive(Debug)]
pub enum Message {
    Output(String),
    Done(Action, Result<(), String>, Option<Overview>),
}
fn stream<R: std::io::Read + Send + 'static>(
    source: R,
    tx: Sender<Message>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(source).lines() {
            match line {
                Ok(line) => {
                    let _ = tx.send(Message::Output(line));
                }
                Err(_) => break,
            }
        }
    })
}
fn run_privileged(action: Action, tx: &Sender<Message>) -> Result<(), String> {
    // Never interpolate package names, shell snippets, or user-editable commands.
    // pkexec prompts via the user's registered Polkit authentication agent.
    let mut cmd = Command::new("/usr/bin/pkexec");
    cmd.args(privileged_args(action))
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| {
        format!("Could not start Polkit authorization: {e}. Ensure pkexec is installed.")
    })?;
    let stdout = stream(
        child.stdout.take().ok_or("Missing stdout pipe")?,
        tx.clone(),
    );
    let stderr = stream(
        child.stderr.take().ok_or("Missing stderr pipe")?,
        tx.clone(),
    );
    let exit = child
        .wait()
        .map_err(|e| format!("APT process failed: {e}"))?;
    let _ = stdout.join();
    let _ = stderr.join();
    if exit.success() {
        Ok(())
    } else if exit.code() == Some(126) {
        Err("Authentication was cancelled; no operation was performed by zSoft Manager.".into())
    } else {
        Err(format!(
            "APT operation failed (exit {}). See operation log for details.",
            exit.code().map_or("unknown".into(), |c| c.to_string())
        ))
    }
}
/// Called on a background worker only. The approved preview is revalidated before privilege.
/// APT itself performs its own dependency/safety checks after acquiring the dpkg lock.
pub fn execute(action: Action, approved: Option<Plan>, tx: Sender<Message>) {
    let result = (|| {
        if matches!(action, Action::Upgrade | Action::Autoremove) {
            let old = approved.ok_or("A confirmed preview is required for this operation")?;
            if old.action != action {
                return Err("Preview/action mismatch".into());
            }
            validate_plan(&old)?;
            let latest = preview(action)?;
            if latest != old {
                return Err(
                    "APT package state changed since the preview. Review again before continuing."
                        .into(),
                );
            }
            if action == Action::Upgrade && latest.install.is_empty() {
                return Err("No upgradeable packages in the current APT plan".into());
            }
            if action == Action::Autoremove && latest.remove.is_empty() {
                return Err("No unused packages to remove".into());
            }
        }
        run_privileged(action, &tx)
    })();
    let refreshed = if result.is_ok() {
        overview().ok()
    } else {
        None
    };
    let _ = tx.send(Message::Done(action, result, refreshed));
}
#[cfg(test)]
mod tests {
    use super::*;
    const UPGRADE:&str="Reading package lists... Done\nThe following packages will be upgraded:\n foo\n1 upgraded, 0 newly installed, 0 to remove and 2 not upgraded.\nInst foo [1.0] (1.1 Ubuntu:stable [amd64])\nConf foo (1.1 Ubuntu:stable [amd64])\n";
    #[test]
    fn preview_parses_versions_and_held_summary() {
        let p = parse_action(UPGRADE, Action::Upgrade).unwrap();
        assert_eq!(p.install[0].name, "foo");
        assert_eq!(p.install[0].installed, "1.0");
        assert_eq!(p.install[0].candidate, "1.1");
        assert!(p.summary.contains("2 not upgraded"));
    }
    #[test]
    fn refuses_removals_during_upgrade() {
        let p = parse_action(
            "0 upgraded, 0 newly installed, 1 to remove and 0 not upgraded.\nRemv foo [1]\n",
            Action::Upgrade,
        )
        .unwrap();
        assert!(validate_plan(&p).is_err());
    }
    #[test]
    fn refuses_critical_autoremove() {
        for n in [
            "linux-image-6.8.0",
            "ubuntu-desktop",
            "systemd",
            "grub-efi-amd64",
            "nvidia-driver-550",
        ] {
            let p=parse_action(&format!("0 upgraded, 0 newly installed, 1 to remove and 0 not upgraded.\nRemv {n} [1]\n"),Action::Autoremove).unwrap();
            assert!(validate_plan(&p).is_err());
        }
    }
    #[test]
    fn safe_autoremove_is_allowed() {
        let p=parse_action("0 upgraded, 0 newly installed, 1 to remove and 0 not upgraded.\nRemv old-library [1]\n",Action::Autoremove).unwrap();
        assert!(validate_plan(&p).is_ok());
    }
    #[test]
    fn fixed_authorized_commands() {
        for action in [
            Action::Refresh,
            Action::Upgrade,
            Action::Autoremove,
            Action::Autoclean,
        ] {
            let args = privileged_args(action);
            assert_eq!(args[0], "/usr/bin/apt-get");
            assert!(!args.iter().any(|s| s.contains("sh -c")
                || s.contains("--allow-unauthenticated")
                || s.contains("--force-yes")));
        }
        assert!(privileged_args(Action::Upgrade).contains(&"--no-remove"));
    }
    #[test]
    fn rejects_unrecognized_simulation() {
        assert!(parse_action("garbled", Action::Upgrade).is_err());
    }
    #[test]
    fn rejects_partial_removal_simulation() {
        assert!(parse_action(
            "0 upgraded, 0 newly installed, 2 to remove and 0 not upgraded.\nRemv foo [1]\n",
            Action::Autoremove
        )
        .is_err());
    }
}
