//! Is there a newer release, and `gitcloner --update` to fetch it.
//!
//! The check reads where `github.com/<repo>/releases/latest` redirects to,
//! through `curl`, so it costs no GitHub API quota and needs no token.
//! `GITCLONER_NO_UPDATE_CHECK=1` turns it off.

use std::fs;
use std::process::{Command, ExitCode, Stdio};
use std::sync::OnceLock;
use std::thread;

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const LATEST_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest");
const PS1_URL: &str =
    concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest/download/gitcloner-installer.ps1");
const SH_URL: &str =
    concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest/download/gitcloner-installer.sh");

/// A newer version than this one, once the background check has found it.
static NEWER: OnceLock<String> = OnceLock::new();

/// The installer one-liner for this platform, to paste into a shell.
pub fn install_command() -> String {
    if cfg!(windows) {
        format!("powershell -ExecutionPolicy Bypass -c \"irm {PS1_URL} | iex\"")
    } else {
        format!("curl --proto '=https' --tlsv1.2 -LsSf {SH_URL} | sh")
    }
}

pub fn available() -> Option<&'static str> {
    NEWER.get().map(String::as_str)
}

/// Look for a newer release off-thread; `available()` answers once it lands.
pub fn spawn_check() {
    if std::env::var_os("GITCLONER_NO_UPDATE_CHECK").is_some() {
        return;
    }
    thread::spawn(|| {
        if let Some(v) = latest().filter(|v| newer(v, CURRENT)) {
            let _ = NEWER.set(v);
        }
    });
}

/// The newest released version, e.g. `0.2.0`. `None` when offline, when curl
/// is missing, or when there is no release yet.
fn latest() -> Option<String> {
    let out = Command::new("curl")
        .args(["--silent", "--head", "--max-time", "5", "--write-out", r"\n%{redirect_url}"])
        .arg(LATEST_URL)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (_, tag) = text.rsplit('\n').next()?.trim().rsplit_once("/releases/tag/")?;
    let version = tag.trim_start_matches('v');
    (!version.is_empty()).then(|| version.to_string())
}

/// `0.10.0` beats `0.9.3`; anything after `-` or `+` is ignored.
fn newer(latest: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        let core = v.trim_start_matches('v').split(['-', '+']).next().unwrap_or("");
        core.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    parts(latest) > parts(current)
}

/// `gitcloner --update`: run the release installer for this platform.
pub fn run() -> ExitCode {
    match latest() {
        Some(v) if !newer(&v, CURRENT) => {
            println!("gitcloner {CURRENT} is already the latest version.");
            return ExitCode::SUCCESS;
        }
        Some(v) => println!("updating gitcloner {CURRENT} -> {v}\n"),
        None => println!("could not see the latest version; running the installer anyway\n"),
    }

    // Windows will not overwrite a running .exe, but it will rename one: park
    // this binary so the installer can put the new one in its place.
    let parked = std::env::current_exe().ok().filter(|_| cfg!(windows)).and_then(|exe| {
        let old = exe.with_extension("old.exe");
        let _ = fs::remove_file(&old);
        fs::rename(&exe, &old).ok().map(|()| (exe, old))
    });
    let status = if cfg!(windows) {
        Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
            .arg(format!("irm {PS1_URL} | iex"))
            .status()
    } else {
        Command::new("sh").args(["-c", &install_command()]).status()
    };
    // The installer put the new binary somewhere else, or failed: put us back.
    if let Some((exe, old)) = parked.filter(|(exe, _)| !exe.exists()) {
        let _ = fs::rename(&old, &exe);
    }

    if status.is_ok_and(|s| s.success()) {
        ExitCode::SUCCESS
    } else {
        eprintln!("\ngitcloner: the update did not finish. Run the installer yourself:\n\n    {}\n", install_command());
        ExitCode::FAILURE
    }
}

/// What `--update` leaves behind on Windows; gone on the next start. A no-op
/// everywhere else.
pub fn tidy() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(exe.with_extension("old.exe"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.3"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(!newer("0.2.0-beta.1", "0.2.0"));
    }
}
