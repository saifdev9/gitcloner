//! Running `git clone` and making sense of what comes back on stderr.
//!
//! Credentials are fed to git through `GIT_ASKPASS` pointing back at this very
//! binary (see `main::askpass`), so a PAT never appears in a command line, in
//! `.git/config`, or in the process table.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::repo::{self, Kind, RepoSpec};

pub const ENV_ASKPASS: &str = "GITCLONER_ASKPASS";
pub const ENV_USER: &str = "GITCLONER_ASKPASS_USER";
pub const ENV_TOKEN: &str = "GITCLONER_ASKPASS_TOKEN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnExisting {
    Skip,
    Rename,
    Overwrite,
}

impl OnExisting {
    pub fn label(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Rename => "rename",
            Self::Overwrite => "overwrite",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Skip => Self::Rename,
            Self::Rename => Self::Overwrite,
            Self::Overwrite => Self::Skip,
        }
    }

    pub fn prev(self) -> Self {
        self.next().next()
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub spec: RepoSpec,
    /// Directory to create. May be renamed when it already exists.
    pub dest: PathBuf,
    pub branch: Option<String>,
    pub shallow: bool,
    pub submodules: bool,
    pub user: Option<String>,
    pub token: Option<String>,
    pub on_existing: OnExisting,
}

#[derive(Debug)]
pub enum Update {
    Phase(String),
    Progress(u16),
    Log(String),
}

#[derive(Debug)]
pub enum Outcome {
    Cloned(PathBuf),
    Skipped(String),
    Failed(String),
    Cancelled,
}

/// Lets the UI thread cancel a running clone.
#[derive(Clone, Default)]
pub struct Handle {
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl Handle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let mut guard = match self.child.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(child) = guard.as_mut() {
            kill_tree(child);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// `git clone` spawns helper processes (git-remote-https, ssh); killing only
/// the parent can leave them writing into the directory we are about to
/// delete, so take the whole tree down on Windows.
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

pub fn git_version() -> Result<String, String> {
    let out = Command::new("git").arg("--version").output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "git is not on PATH. Install Git (https://git-scm.com/downloads), reopen the terminal and try again.".to_string()
        } else {
            format!("could not run git: {e}")
        }
    })?;
    if !out.status.success() {
        return Err("`git --version` failed — the git install looks broken".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Throwaway repos on disk, shared by the test modules in this crate.
#[cfg(test)]
pub(crate) mod testing {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub fn git(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git must be installed to run these tests");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    pub fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("gitcloner-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp dir");
        root
    }

    /// A repo with one commit on `main`.
    pub fn make_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "-b", "main", "--quiet"]);
        fs::write(path.join("README.md"), "hello").unwrap();
        git(path, &["add", "."]);
        git(
            path,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-m",
                "first",
                "--quiet",
            ],
        );
    }
}

// ---------------------------------------------------------------- destination

enum Dest {
    Use { path: PathBuf, created: bool },
    Skip(String),
}

fn show(p: &Path) -> String {
    p.display().to_string()
}

/// Never hand `remove_dir_all` a drive root, or anything the current directory
/// lives inside.
fn safe_to_remove(path: &Path) -> bool {
    let Ok(abs) = path.canonicalize() else { return false };
    if abs.file_name().is_none() || abs.components().count() < 3 {
        return false;
    }
    // Both sides must be canonical: on Windows `canonicalize` adds a `\\?\`
    // prefix, and a raw cwd would never match an absolute path that has one.
    match std::env::current_dir().and_then(|cwd| cwd.canonicalize()) {
        Ok(cwd) => !cwd.starts_with(&abs),
        Err(_) => false,
    }
}

fn next_free(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| format!("{} has no folder name", show(path)))?;
    let parent = path.parent().unwrap_or(Path::new("."));
    for n in 2..100 {
        let candidate = parent.join(format!("{name}-{n}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(format!("{name}-2 .. {name}-99 are all taken"))
}

fn prepare_dest(job: &Job) -> Result<Dest, String> {
    let mut path = job.dest.clone();
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if !meta.is_dir() {
            let what = if meta.is_symlink() { "a symlink" } else { "a file" };
            return Err(format!("{} already exists and is {what}", show(&path)));
        }
        let mut entries =
            fs::read_dir(&path).map_err(|e| format!("cannot read {}: {e}", show(&path)))?;
        if entries.next().is_some() {
            match job.on_existing {
                OnExisting::Skip => {
                    let git_dir = path.join(".git").exists();
                    let why = if git_dir { "already cloned" } else { "folder not empty" };
                    return Ok(Dest::Skip(format!("{why}: {}", show(&path))));
                }
                OnExisting::Rename => path = next_free(&path)?,
                OnExisting::Overwrite => {
                    if !safe_to_remove(&path) {
                        return Err(format!("refusing to delete {}", show(&path)));
                    }
                    fs::remove_dir_all(&path)
                        .map_err(|e| format!("could not replace {}: {e}", show(&path)))?;
                }
            }
        }
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", show(parent)))?;
    }
    let created = !path.exists();
    Ok(Dest::Use { path, created })
}

// -------------------------------------------------------------------- progress

pub struct Prog {
    pub phase: &'static str,
    pub pct: Option<u16>,
}

/// (stderr prefix, label shown, where the phase starts, how much it is worth)
const PHASES: &[(&str, &str, f32, f32)] = &[
    ("Enumerating objects", "enumerating", 0.0, 2.0),
    ("Counting objects", "counting", 2.0, 3.0),
    ("Compressing objects", "compressing", 5.0, 5.0),
    ("Receiving objects", "receiving", 10.0, 70.0),
    ("Resolving deltas", "resolving deltas", 80.0, 10.0),
    ("Updating files", "checking out", 90.0, 10.0),
    ("Checking out files", "checking out", 90.0, 10.0),
    ("Filtering content", "git-lfs", 95.0, 5.0),
    ("Submodule path", "submodules", 96.0, 4.0),
    ("Cloning into", "connecting", 0.0, 0.0),
];

fn percent_in(s: &str) -> Option<u16> {
    let idx = s.find('%')?;
    let digits: String = s[..idx].chars().rev().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.chars().rev().collect::<String>().parse().ok().map(|p: u16| p.min(100))
}

/// Map one line of `git clone --progress` output onto an overall percentage.
pub fn parse_progress(line: &str) -> Option<Prog> {
    let line = line.trim();
    let body = line.strip_prefix("remote: ").unwrap_or(line);
    let (_, label, start, span) = PHASES.iter().find(|(prefix, ..)| body.starts_with(prefix))?;
    let pct = percent_in(body).map(|p| (start + span * f32::from(p) / 100.0).round() as u16);
    Some(Prog { phase: label, pct })
}

// -------------------------------------------------------------- error messages

const RULES: &[(&[&str], &str)] = &[
    (
        &["could not read username", "terminal prompts disabled", "authentication required"],
        "credentials needed — the repo is private (or does not exist): add a PAT",
    ),
    (
        &["authentication failed", "invalid username or password", "bad credentials", "403"],
        "PAT rejected — check it has not expired and covers this repo (repo / read_repository scope)",
    ),
    // Before the greedy "not found" rule below.
    (&["remote branch", "unknown revision", "pathspec"], "branch or tag not found on the remote"),
    (
        &["repository not found", "not found", "404", "does not appear to be a git repository"],
        "repository not found — check the URL, or add a PAT if it is private",
    ),
    (
        &["permission denied (publickey)", "permission denied, please try again"],
        "SSH key rejected by the host",
    ),
    (
        &["host key verification failed", "no matching host key"],
        "unknown SSH host key — clone once from the shell to trust the host",
    ),
    (
        &["could not resolve host", "failed to connect", "connection timed out", "network is unreachable", "connection refused", "could not resolve proxy"],
        "network problem — could not reach the host",
    ),
    (
        &["ssl certificate problem", "unable to get local issuer", "certificate verify failed", "schannel"],
        "TLS problem — the certificate was rejected (corporate proxy?)",
    ),
    (&["already exists and is not an empty directory"], "destination folder is not empty"),
    (
        &["filename too long", "unable to create file", "invalid path"],
        "a path in this repo is too long or illegal here — clone nearer the drive root (or enable long paths)",
    ),
    (&["no space left", "not enough space"], "out of disk space"),
    (
        &["early eof", "index-pack failed", "rpc failed", "the remote end hung up", "fetch-pack"],
        "transfer broke part-way — try again, or turn shallow on",
    ),
    (
        &["does not support shallow", "shallow update not allowed"],
        "the server refused a shallow clone — turn depth off",
    ),
    (&["dubious ownership"], "git refuses this path (dubious ownership)"),
    (&["lfs"], "git-lfs step failed"),
];

/// Turn git's stderr into one line a human can act on.
pub fn classify(stderr: &str, code: Option<i32>) -> String {
    let lower = stderr.to_ascii_lowercase();
    let detail = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| {
            let l = l.to_ascii_lowercase();
            l.starts_with("fatal:") || l.starts_with("error:") || l.starts_with("remote: error")
        })
        .unwrap_or("")
        .to_string();
    for (needles, message) in RULES {
        if needles.iter().any(|n| lower.contains(n)) {
            return if detail.is_empty() {
                (*message).to_string()
            } else {
                format!("{message} — git: {detail}")
            };
        }
    }
    if !detail.is_empty() {
        return detail;
    }
    match code {
        Some(c) => format!("git exited with code {c}"),
        None => "git was stopped".into(),
    }
}

// ------------------------------------------------------------------ the runner

fn build_command(job: &Job, dest: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(["-c", "advice.detachedHead=false"]);
    #[cfg(windows)]
    cmd.args(["-c", "core.longpaths=true"]);
    if job.token.is_some() {
        // Empty value resets the helper chain, so the OS credential manager
        // cannot pop a dialog or override our token.
        cmd.args(["-c", "credential.helper="]);
    }
    cmd.args(["clone", "--progress"]);
    if job.shallow {
        cmd.args(["--depth", "1", "--single-branch"]);
    }
    if let Some(branch) = job.branch.as_deref().filter(|b| !b.is_empty()) {
        cmd.args(["--branch", branch]);
    }
    if job.submodules {
        cmd.arg("--recurse-submodules");
        if job.shallow {
            cmd.arg("--shallow-submodules");
        }
    }
    cmd.arg("--").arg(&job.spec.url).arg(dest);

    // Nothing may ever block waiting for a human: no stdin, no terminal
    // prompt, no interactive ssh.
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    // Keep git's messages in English so the error rules above still match.
    cmd.env("LC_ALL", "C").env("LANG", "C");
    if job.spec.kind == Kind::Ssh {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    match (&job.token, std::env::current_exe()) {
        (Some(token), Ok(exe)) => {
            let user = job
                .user
                .clone()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| repo::default_user(&job.spec.host).to_string());
            cmd.env(ENV_ASKPASS, "1")
                .env(ENV_USER, user)
                .env(ENV_TOKEN, token)
                .env("GIT_ASKPASS", exe);
        }
        _ => {
            cmd.env_remove(ENV_ASKPASS);
            cmd.env_remove(ENV_TOKEN);
        }
    }
    cmd
}

/// Clone one repo, start to finish. Blocks; meant to be run on its own thread.
/// `emit` is called with progress as it arrives.
pub fn run_job(job: &Job, handle: &Handle, emit: &mut dyn FnMut(Update)) -> Outcome {
    let (dest, created) = match prepare_dest(job) {
        Ok(Dest::Use { path, created }) => (path, created),
        Ok(Dest::Skip(why)) => return Outcome::Skipped(why),
        Err(e) => return Outcome::Failed(e),
    };
    if handle.is_cancelled() {
        return Outcome::Cancelled;
    }
    emit(Update::Log(format!("git clone -> {}", show(&dest))));

    let mut child = match build_command(job, &dest).spawn() {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("could not start git: {e}")),
    };
    let stderr = child.stderr.take();
    match handle.child.lock() {
        Ok(mut slot) => *slot = Some(child),
        Err(poisoned) => *poisoned.into_inner() = Some(child),
    }
    if handle.is_cancelled() {
        handle.cancel(); // the child arrived after a cancel; stop it now
    }

    let mut collected = String::new();
    if let Some(mut stderr) = stderr {
        let token = job.token.as_deref();
        let mut buf = [0u8; 2048];
        let mut line: Vec<u8> = Vec::new();
        let mut last_pct = 0u16;
        loop {
            let n = match stderr.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            for &byte in &buf[..n] {
                // git separates progress updates with \r and messages with \n
                if byte != b'\r' && byte != b'\n' {
                    line.push(byte);
                    continue;
                }
                if line.is_empty() {
                    continue;
                }
                let text = repo::redact(&String::from_utf8_lossy(&line), token);
                line.clear();
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                match parse_progress(&trimmed) {
                    Some(prog) => {
                        emit(Update::Phase(prog.phase.to_string()));
                        if let Some(pct) = prog.pct.filter(|p| *p != last_pct) {
                            last_pct = pct;
                            emit(Update::Progress(pct));
                        }
                    }
                    None => emit(Update::Log(trimmed.clone())),
                }
                collected.push_str(&trimmed);
                collected.push('\n');
            }
        }
        if !line.is_empty() {
            let text = repo::redact(&String::from_utf8_lossy(&line), token);
            emit(Update::Log(text.trim().to_string()));
            collected.push_str(text.trim());
            collected.push('\n');
        }
    }

    // stderr is closed, so git is on its way out; poll instead of blocking so
    // a cancel can still get the lock.
    let status = loop {
        let mut guard = match handle.child.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match guard.as_mut().map(Child::try_wait) {
            Some(Ok(Some(status))) => break Some(status),
            Some(Ok(None)) => {}
            _ => break None,
        }
        drop(guard);
        thread::sleep(Duration::from_millis(40));
    };
    if let Ok(mut slot) = handle.child.lock() {
        *slot = None;
    }

    let ok = status.map(|s| s.success()).unwrap_or(false);
    if ok && !handle.is_cancelled() {
        return Outcome::Cloned(dest);
    }
    // A half-written clone is worse than none: clean up, but only what we made.
    if created && dest.exists() && safe_to_remove(&dest) {
        let _ = fs::remove_dir_all(&dest);
    }
    if handle.is_cancelled() {
        return Outcome::Cancelled;
    }
    Outcome::Failed(classify(&collected, status.and_then(|s| s.code())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines() {
        let p = parse_progress("Receiving objects:  50% (50/100), 1.00 MiB | 2.00 MiB/s").unwrap();
        assert_eq!((p.phase, p.pct), ("receiving", Some(45)));
        let p = parse_progress("remote: Compressing objects: 100% (30/30), done.").unwrap();
        assert_eq!((p.phase, p.pct), ("compressing", Some(10)));
        let p = parse_progress("Resolving deltas: 100% (12/12), done.").unwrap();
        assert_eq!((p.phase, p.pct), ("resolving deltas", Some(90)));
        let p = parse_progress("remote: Enumerating objects: 118, done.").unwrap();
        assert_eq!((p.phase, p.pct), ("enumerating", None));
        assert!(parse_progress("fatal: repository not found").is_none());
    }

    #[test]
    fn error_messages_are_actionable() {
        let cases = [
            ("fatal: could not read Username for 'https://github.com': terminal prompts disabled", "credentials needed"),
            ("remote: Invalid username or password.\nfatal: Authentication failed", "PAT rejected"),
            ("remote: Repository not found.\nfatal: repository not found", "repository not found"),
            ("fatal: unable to access 'https://x/': Could not resolve host: x", "network problem"),
            ("fatal: Remote branch nope not found in upstream origin", "branch or tag not found"),
            ("git@github.com: Permission denied (publickey).", "SSH key rejected"),
            ("error: unable to create file very/long/path: Filename too long", "too long or illegal"),
        ];
        for (stderr, expected) in cases {
            let msg = classify(stderr, Some(128));
            assert!(msg.contains(expected), "{stderr:?} gave {msg:?}");
        }
        assert_eq!(classify("", Some(9)), "git exited with code 9");
    }

    #[test]
    fn urls_and_tokens_stay_out_of_the_command() {
        let spec = crate::repo::parse("https://github.com/owner/repo").unwrap();
        let job = Job {
            spec,
            dest: PathBuf::from("/tmp/repo"),
            branch: Some("main".into()),
            shallow: true,
            submodules: false,
            user: None,
            token: Some("ghp_secret".into()),
            on_existing: OnExisting::Skip,
        };
        let cmd = build_command(&job, Path::new("/tmp/repo"));
        let args: Vec<String> =
            cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"--depth".to_string()) && args.contains(&"main".to_string()));
        assert!(!args.iter().any(|a| a.contains("ghp_secret")), "token leaked into argv");
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.iter().any(|(k, _)| *k == ENV_TOKEN));
    }

    // ---------------------------------------------------------- end to end

    use super::testing::{make_repo, temp_root};

    /// Clones a repo we just made on disk, so the whole pipeline — argv,
    /// stderr parsing, destination handling, cleanup — runs for real without
    /// needing a network or a token.
    #[test]
    fn clones_a_real_repo_and_handles_the_awkward_cases() {
        let root = temp_root("e2e");
        let source = root.join("source");
        make_repo(&source);

        let make = |src: &Path, dest: &str, on_existing: OnExisting, branch: Option<&str>| Job {
            spec: crate::repo::parse(&src.display().to_string()).expect("local path parses"),
            dest: root.join(dest),
            branch: branch.map(str::to_string),
            shallow: false,
            submodules: false,
            user: None,
            token: None,
            on_existing,
        };
        let handle = Handle::default();
        let log = std::cell::RefCell::new(Vec::<String>::new());
        let mut emit = |u: Update| log.borrow_mut().push(format!("{u:?}"));

        // a clean clone
        match run_job(&make(&source, "copy", OnExisting::Skip, None), &handle, &mut emit) {
            Outcome::Cloned(path) => assert!(path.join("README.md").exists(), "working tree"),
            other => panic!("expected a clone, got {other:?}\n{log:#?}"),
        }
        let seen = log.borrow().iter().any(|l| l.contains("Progress") || l.contains("Phase"));
        assert!(seen, "no progress was reported: {:#?}", log.borrow());

        // second run: the folder is already there
        match run_job(&make(&source, "copy", OnExisting::Skip, None), &handle, &mut emit) {
            Outcome::Skipped(why) => assert!(why.contains("already cloned"), "{why}"),
            other => panic!("expected a skip, got {other:?}"),
        }

        // rename instead of skipping
        match run_job(&make(&source, "copy", OnExisting::Rename, None), &handle, &mut emit) {
            Outcome::Cloned(path) => assert!(path.ends_with("copy-2"), "{}", path.display()),
            other => panic!("expected copy-2, got {other:?}"),
        }

        // overwrite replaces what is there
        fs::write(root.join("copy").join("stale.txt"), "x").unwrap();
        match run_job(&make(&source, "copy", OnExisting::Overwrite, None), &handle, &mut emit) {
            Outcome::Cloned(path) => assert!(!path.join("stale.txt").exists(), "not replaced"),
            other => panic!("expected an overwrite, got {other:?}"),
        }

        // a branch that does not exist: useful message, no leftover folder
        match run_job(&make(&source, "wrong-branch", OnExisting::Skip, Some("nope")), &handle, &mut emit) {
            Outcome::Failed(why) => assert!(why.contains("branch or tag not found"), "{why}"),
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(!root.join("wrong-branch").exists(), "failed clone left a folder behind");

        // a path that is not a repo at all
        let empty = root.join("not-a-repo");
        fs::create_dir_all(&empty).unwrap();
        match run_job(&make(&empty, "nope", OnExisting::Skip, None), &handle, &mut emit) {
            Outcome::Failed(why) => assert!(why.contains("repository not found"), "{why}"),
            other => panic!("expected a failure, got {other:?}"),
        }

        // a destination that is a file, not a folder
        let file_dest = root.join("afile");
        fs::write(&file_dest, "x").unwrap();
        match run_job(&make(&source, "afile", OnExisting::Skip, None), &handle, &mut emit) {
            Outcome::Failed(why) => assert!(why.contains("is a file"), "{why}"),
            other => panic!("expected a failure, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&root);
    }

    /// Needs the network: `cargo test -- --ignored`.
    #[test]
    #[ignore = "hits github.com"]
    fn network_clone_and_auth_paths() {
        let root = temp_root("net");
        let job = |url: &str, dest: &str, token: Option<&str>| Job {
            spec: crate::repo::parse(url).unwrap(),
            dest: root.join(dest),
            branch: None,
            shallow: true,
            submodules: false,
            user: None,
            token: token.map(str::to_string),
            on_existing: OnExisting::Skip,
        };
        let handle = Handle::default();
        let pct = std::cell::Cell::new(0u16);
        let mut emit = |u: Update| {
            if let Update::Progress(p) = u {
                pct.set(p);
            }
        };

        // a public repo, no credentials involved
        match run_job(&job("https://github.com/octocat/Hello-World", "hello", None), &handle, &mut emit) {
            Outcome::Cloned(path) => assert!(path.join(".git").is_dir()),
            other => panic!("public clone failed: {other:?}"),
        }
        assert!(pct.get() > 0, "no progress came through");

        // a repo that is private or absent, with a token that is not valid:
        // this exercises the GIT_ASKPASS round trip
        let bad = job(
            "https://github.com/octocat/gitcloner-does-not-exist",
            "nope",
            Some("ghp_0000000000000000000000000000000000000"),
        );
        match run_job(&bad, &handle, &mut emit) {
            Outcome::Failed(why) => assert!(
                why.contains("PAT rejected")
                    || why.contains("repository not found")
                    || why.contains("credentials needed"),
                "unhelpful message: {why}"
            ),
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(!root.join("nope").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cancelled_clone_reports_cancelled() {
        let handle = Handle::default();
        handle.cancel(); // cancelled before it ever starts
        let root = temp_root("cancel");
        let job = Job {
            spec: crate::repo::parse("https://github.com/owner/repo").unwrap(),
            dest: root.join("repo"),
            branch: None,
            shallow: true,
            submodules: false,
            user: None,
            token: None,
            on_existing: OnExisting::Skip,
        };
        let mut emit = |_: Update| {};
        assert!(matches!(run_job(&job, &handle, &mut emit), Outcome::Cancelled));
        assert!(!root.join("repo").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_to_delete_dangerous_paths() {
        assert!(!safe_to_remove(Path::new("/")));
        assert!(!safe_to_remove(Path::new(".")));
        assert!(!safe_to_remove(Path::new("does/not/exist")));
    }
}
