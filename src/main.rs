//! gitcloner — a terminal UI for cloning a pile of git repos, public or private.

mod app;
mod art;
mod clone;
mod org;
mod repo;
mod settings;
mod ui;
mod update;

use std::io::{self, Write};
use std::panic;
use std::process::ExitCode;

use ratatui::crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::execute;
use ratatui::crossterm::tty::IsTty;

const HELP: &str = concat!("\
gitcloner — clone many git repos, public or private, from one screen

USAGE:
    gitcloner [URL...] [OPTIONS]

    With no arguments it opens the UI with an empty list. Any URLs given are
    pre-loaded into it. Nothing is cloned until you press ctrl+s.

URL FORMS:
    https://github.com/owner/repo[.git]      owner/repo
    https://github.com/owner/repo/tree/dev   (branch is picked up)
    https://gitlab.com/group/sub/repo/-/tree/main
    git@github.com:owner/repo.git            ssh://git@host:port/owner/repo
    /path/to/local/repo                      file:///path/to/repo
    https://github.com/myorg                 (lists the org's repos to pick from)

OPTIONS:
    -d, --dest <DIR>      where to clone into (default: current directory)
    -b, --branch <NAME>   branch to check out
    -u, --user <NAME>     username for the token (default: per host)
        --shallow        --depth 1 --single-branch
        --update          update gitcloner to the latest release
    -h, --help            this text
    -V, --version         version

For private repos, paste a personal access token into the form, or set
$GITCLONER_PAT (any https host), $GITHUB_TOKEN / $GH_TOKEN (github.com only)
or $GITLAB_TOKEN (gitlab.com only). Tokens are passed to git through
GIT_ASKPASS, only for the host being cloned and only over https, so they never
land in a command line or in .git/config.

KEYS:
    tab / shift+tab   move between fields       ctrl+s   start cloning
    ctrl+t            show / hide the token     ctrl+u   clear the field
    ↑↓ or j/k         pick a repo               c / C    cancel one / all
    r / R             retry one / all failed    b        back to the form
    space             tick a repo (org list)    a        tick all / none
    q / esc           quit
    F1 or ctrl+g      help                      F2 or ctrl+o   settings (theme…)
    F3 or ctrl+a      profile (your GitHub account, and who made gitcloner)

Settings are saved to %APPDATA%\\gitcloner\\settings.json on Windows and
~/.config/gitcloner/settings.json elsewhere. Tokens are never saved.

On start it checks in the background for a newer release and, if there is
one, says how to update when you quit. GITCLONER_NO_UPDATE_CHECK=1 turns
that off.

made by ", env!("CARGO_PKG_AUTHORS"), " · ", env!("CARGO_PKG_REPOSITORY"), "
");

fn main() -> ExitCode {
    // Re-entrant mode: git calls this binary back to ask for credentials.
    if std::env::var_os(clone::ENV_ASKPASS).is_some() {
        return askpass();
    }
    if std::env::args().skip(1).any(|a| a == "--update") {
        return update::run();
    }
    update::tidy();

    let prefill = match parse_args() {
        Ok(Some(prefill)) => prefill,
        Ok(None) => return ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("gitcloner: {msg}\n\n{HELP}");
            return ExitCode::FAILURE;
        }
    };

    let git = match clone::git_version() {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("gitcloner: {msg}");
            return ExitCode::FAILURE;
        }
    };

    // Without a terminal on both ends the UI would draw into a pipe and then
    // block forever waiting for keys nobody can send.
    if !io::stdin().is_tty() || !io::stdout().is_tty() {
        eprintln!(
            "gitcloner: this is an interactive UI — run it in a terminal, not through a pipe."
        );
        return ExitCode::FAILURE;
    }

    let mut terminal = match ratatui::try_init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("gitcloner: cannot start the UI ({e}). It needs a real terminal.");
            return ExitCode::FAILURE;
        }
    };
    // ratatui's hook restores the screen on panic; also turn paste back off.
    let hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        hook(info);
    }));
    let _ = execute!(io::stdout(), EnableBracketedPaste);

    let result = app::App::new(prefill, git, settings::Settings::load()).run(&mut terminal);

    let _ = execute!(io::stdout(), DisableBracketedPaste);
    ratatui::restore();

    // The UI said so too; this copy stays on screen, ready to paste.
    if let Some(v) = update::available() {
        eprintln!(
            "\n  gitcloner {v} is out — you have {}. Update with:\n\n    gitcloner --update\n\n  or paste:\n\n    {}\n",
            env!("CARGO_PKG_VERSION"),
            update::install_command()
        );
    }

    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("gitcloner: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `Ok(None)` means "already printed what was asked for, exit cleanly".
fn parse_args() -> Result<Option<app::Prefill>, String> {
    let mut prefill = app::Prefill::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("{name} needs a value"))
        };
        match arg {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!(
                    "gitcloner {} — made by {}",
                    env!("CARGO_PKG_VERSION"),
                    env!("CARGO_PKG_AUTHORS")
                );
                return Ok(None);
            }
            "-d" | "--dest" => prefill.dest = Some(value("--dest")?),
            "-b" | "--branch" => prefill.branch = Some(value("--branch")?),
            "-u" | "--user" => prefill.user = Some(value("--user")?),
            // gitcloner stays open, so a token here would sit in `ps` output
            // (and shell history) for the whole session.
            "-t" | "--token" | "--pat" => {
                return Err(format!(
                    "{arg} is gone: a token on the command line is visible to other users in ps. \
                     Set GITCLONER_PAT instead, or paste it into the token field."
                ));
            }
            "--shallow" | "--depth1" => prefill.shallow = true,
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option {other}"));
            }
            other => prefill.urls.extend(repo::split_inputs(other)),
        }
        i += 1;
    }
    Ok(Some(prefill))
}

/// git runs us as `GIT_ASKPASS <prompt>`; answer with one line on stdout.
fn askpass() -> ExitCode {
    let prompt = std::env::args().nth(1).unwrap_or_default().to_ascii_lowercase();
    // Only the host the clone was started against gets an answer: a submodule
    // or redirect pointing somewhere else must not be handed the token.
    let asked = prompt.split('\'').nth(1).map(clone::origin).unwrap_or_default();
    if asked.is_empty() || std::env::var(clone::ENV_ORIGIN).ok().as_deref() != Some(asked.as_str()) {
        return ExitCode::FAILURE;
    }
    let key = if prompt.contains("username") {
        clone::ENV_USER
    } else if prompt.contains("password") {
        clone::ENV_TOKEN
    } else {
        // Not a credential prompt (host keys, passphrases): refuse, so git
        // fails fast instead of being handed the wrong secret.
        return ExitCode::FAILURE;
    };
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => {
            let mut out = io::stdout();
            let _ = writeln!(out, "{value}");
            let _ = out.flush();
            ExitCode::SUCCESS
        }
        _ => ExitCode::FAILURE,
    }
}
