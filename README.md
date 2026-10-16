# gitcloner

A terminal UI ([ratatui](https://ratatui.rs/)) for cloning a pile of git repos in one go —
public or private, several at a time, with a PAT only when one is needed.

Made by [saifdev9](https://github.com/saifdev9) · Website: [saifdev9.github.io/gitcloner](https://saifdev9.github.io/gitcloner/)

## Install

macOS / Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/saifdev9/gitcloner/releases/latest/download/gitcloner-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/saifdev9/gitcloner/releases/latest/download/gitcloner-installer.ps1 | iex"
```

Or download the archive for your platform from
[Releases](https://github.com/saifdev9/gitcloner/releases). It needs `git` installed;
listing an org's repos also uses `curl`, which macOS, Windows 10+ and most Linux
systems already have. Then:

```
gitcloner                                    # open the UI
gitcloner https://github.com/owner/repo owner/other --dest ~/src --shallow
gitcloner https://github.com/myorg           # pick from an org's repos
```

Paste a list of URLs, set a destination, add a token if any of them are private,
press **ctrl+s**. Progress, failures and retries are all on one screen.

```
 gitcloner  • clone many repos — public or private · git 2.51.0   made by saifdev9 · v0.1.0
╭ repositories — one per line (3) ─────────────────────────────────────╮
│ https://github.com/owner/repo                                        │
│ owner/another-repo                                                   │
│ git@github.com:team/private-thing.git                                │
╰──────────────────────────────────────────────────────────────────────╯
╭ destination folder ────────────────────────╮╭ branch ────────────────╮
│ C:\Users\me\src                            ││ default                │
╰────────────────────────────────────────────╯╰────────────────────────╯
╭ username (optional) ──────╮╭ token / PAT  (ctrl+t shows) ────────────╮
│ auto per host             ││ ••••••••••••••••••••                    │
╰───────────────────────────╯╰─────────────────────────────────────────╯
  shallow off   submodules off   if folder exists skip   at once 4
```

The screen is animated: the tagline types itself out, list rows cascade in, running
clones show a moving bar (or a comet while git is still counting), finished ones flash
their mark, and the org list shows a shimmering placeholder while it loads. Each clone
shows its download size and speed as git reports them.

## Update

```
gitcloner --update
```

runs the installer above for your platform and replaces the binary in place (on
Windows too, where a running `.exe` cannot be overwritten — it is moved aside first).

On start, gitcloner checks in the background whether a newer release exists. If one
does, the form says so, and when you quit it prints the commands to paste:

```
  gitcloner 0.2.0 is out — you have 0.1.0. Update with:

    gitcloner --update

  or paste:

    curl --proto '=https' --tlsv1.2 -LsSf https://github.com/saifdev9/gitcloner/releases/latest/download/gitcloner-installer.sh | sh
```

The check is one `curl --head` to `github.com/saifdev9/gitcloner/releases/latest`
(no API quota, no token sent) with a 5 second limit; offline it stays silent. Set
`GITCLONER_NO_UPDATE_CHECK=1` to turn it off.

## Run

Building from source needs Rust 1.85+ (edition 2024); running needs `git` on your `PATH`.

```
# straight from source
cargo run --release                       # open the UI with an empty list
cargo run --release -- --help             # print all options
cargo run --release -- --version

# or build / install once, then call it directly
cargo build --release                     # -> target/release/gitcloner(.exe)
cargo install --path .                    # -> ~/.cargo/bin/gitcloner
gitcloner --help
```

With `cargo run`, everything after `--` goes to gitcloner. With the installed binary,
use `gitcloner` in place of `cargo run --release --`.

```
gitcloner [URL...] [OPTIONS]

gitcloner owner/repo https://github.com/owner/other   # pre-load URLs into the list
gitcloner "a/b, c/d; e/f"                             # one argument can hold a list
gitcloner https://github.com/myorg -t ghp_xxx         # pick from an org's repos (see below)
gitcloner owner/repo -d ~/src                         # -d, --dest <DIR>    (default: current dir)
gitcloner owner/repo -b dev                           # -b, --branch <NAME>
gitcloner owner/repo --shallow                        # --depth 1 --single-branch (alias --depth1)
gitcloner team/private -t ghp_xxx                     # -t, --token, --pat <PAT>
gitcloner team/private -u me -t glpat_xxx             # -u, --user <NAME>   (default: per host)
gitcloner -h                                          # -h, --help
gitcloner -V                                          # -V, --version
gitcloner --update                                    # update to the latest release
```

Pass the token through an environment variable instead, so it stays out of your shell
history. These are checked in order: `GITCLONER_PAT`, `GITHUB_TOKEN`, `GH_TOKEN`, `GITLAB_TOKEN`.

```powershell
# PowerShell
$env:GITHUB_TOKEN = "ghp_xxx"; gitcloner team/private
```

```sh
# bash / zsh
GITHUB_TOKEN=ghp_xxx gitcloner team/private
```

Nothing is cloned until you press **ctrl+s** in the UI. It must run in a real terminal,
so piping its input or output fails. The exit code is 1 if any clone failed.

## Keys

| | |
|---|---|
| `tab` / `shift+tab` | move between fields |
| `ctrl+s` | start cloning |
| `ctrl+t` | show / hide the token |
| `ctrl+u` / `ctrl+w` | clear field / delete word |
| `←` `→` `space` | change a toggle (shallow, submodules, …) |
| `↑` `↓` / `j` `k` | move through the org list / the progress screen |
| `space` / `a` | tick a repo in the org list / tick all or none |
| `enter` | clone what is ticked in the org list |
| `c` / `C` | cancel the selected clone / all of them |
| `r` / `R` | retry the selected one / every failed one |
| `+` `-` | change how many run at once, live |
| `b` | back to the form |
| `q`, `esc`, `ctrl+c` | quit (asks first if clones are running) |
| `F1` / `ctrl+g` | help — every key, by screen |
| `F2` / `ctrl+o` | settings — theme, animations, popup, update check |
| `F3` / `ctrl+a` | profile — your GitHub account, and who made gitcloner |
| `o` / `R` / `b` | in the end-of-batch summary: open the folder / retry / back to form |

The ctrl keys are there for terminals that keep the F-keys for themselves (VS Code's
takes F1). Outside the form, `?` opens help too.

## Start screen

`gitcloner` with no arguments opens on a start screen: a pixel planet turning under
a short menu — Clone repos, Settings, Profile, Help, Quit — with stars twinkling round
it. ↑/↓ and enter choose; esc on the form comes back here. Passing URLs on the command
line skips it, and settings can turn it off.

## Settings, profile and help

**F2 — settings.** Changes apply as you make them and are saved to
`%APPDATA%\gitcloner\settings.json` on Windows, `~/.config/gitcloner/settings.json`
elsewhere. Tokens are never saved.

```
╭ settings ──────────────────────────────────────────────────────────────────╮
│ ▸ theme               ◂ ocean ▸   calm blue                                │
│   animations            on        moving bars, shimmer, lists cascading in │
│   popup when done       on        a summary when a batch of clones ends    │
│   check for updates     on        one quick look at GitHub on start        │
│                                                                            │
│   ███ ███ ███ ███ ███   ━━━━━━━━━━━━━╸────────                             │
╰───────────────────────────────────────── ↑↓ choose · ←→ change · esc close ╯
```

Themes: **ocean** (blue), **mint**, **ember** (orange), **rose**, **mono** (greys only)
and **paper** for terminals with a light background. Turning animations off freezes
every moving part, for slow connections or anyone who prefers a still screen.

**F3 — profile.** With a token set, shows the GitHub account it belongs to — login,
name, repo and follower counts, bio — and **enter** lists your own repos to pick from.
Below that: who made gitcloner, the version, and whether an update is out.

**F1 — help.** Every key on every screen, and the URL forms you can paste.

**When a batch finishes**, a summary pops up: how many cloned, failed or were skipped,
how long it took, where they went, and why each failure failed. `o` opens the folder,
`R` retries the failures, `b` goes back to the form, `enter` closes it. It can be turned
off in settings.

```
╭ ✔ all done ────────────────────────────────────────────────────╮
│                                                                │
│   14 cloned  ·  2 skipped                                      │
│   in 38.4s  →  C:\Users\me\src                                 │
│                                                                │
│    enter close   o open folder   R retry   b back to form      │
╰────────────────────────────────────────────────────────────────╯
```

## Cloning from an org

Put an org (or user) URL in the list — `https://github.com/myorg` or
`https://github.com/orgs/myorg/repositories` — and press **ctrl+s**. gitcloner lists
its repos and opens a tick list:

```
 gitcloner  • myorg — 37 of 42 ticked · about 812.6 MB to fetch · plus 1 other URL(s)
╭───────────────────────────────────────────────────────────────────────────────────╮
│▸● myorg/api        ● Go          ★ 1.2k    48.3 MB    3d ago  private the publi… │
│ ○ myorg/fork-of-x  ● Rust          ★ 7     2.1 MB   8mo ago  fork                │
│ ○ myorg/old-site   ● HTML         ★ 31    906 KB    6y ago  archived 2019 mark… │
│ ● myorg/web        ● TypeScript  ★ 412   131.7 MB  today    the web app         │
╰───────────────────────────────────────────────────────────────────────────────────╯
╭ myorg/api ──────────────────────────────────────────────────────────── private ╮
│the public API, plus the worker that drains the billing queue                      │
│● Go  ·  1.2k stars  ·  87 forks  ·  23 open issues  ·  48.3 MB  ·  license MIT    │
│branch main  ·  last push 3d ago (2026-10-12)  ·  #api #grpc #billing              │
│https://github.com/myorg/api.git                                                   │
╰───────────────────────────────────────────────────────────────────────────────────╯
 ↑↓ move   space tick   a all/none   enter clone ticked   esc back   q quit
```

Repos pasted one by one get the same table: GitHub repos are looked up first (eight at a
time) and start ticked, and each still clones exactly as pasted, so a `/tree/<branch>` in
the link still counts. One that cannot be looked up — private without a token, say —
still clones; its row says why it has no details. Other hosts clone alongside. Turn the
review off in settings (F2) to go straight to cloning.

Each row shows the language, stars, size on GitHub, how long ago it was last pushed and
the repo ID (the number `gh api repos/OWNER/REPO --jq .id` prints);
the panel underneath has everything else GitHub knows about the highlighted repo:
description, forks, open issues, license, default branch, topics and the clone URL.
The title adds up the size of what is ticked, so you know what you are about to pull.

Everything starts ticked except forks and archived repos. **enter** clones the ticked
ones, plus any plain URLs that were in the list, with the same destination, branch and
options as usual.

- Private repos are listed only when the token can see them. For a user account, its
  private repos show up when the token is that user's own.
- Without a token GitHub allows 60 API calls an hour; one call covers 100 repos.
- Listing uses `curl` (built into Windows 10+, Git for Windows, macOS and Linux) and
  hands it the token on stdin, so the token stays off the command line here too.
- GitHub only, for now. A GitLab group URL is still treated as a single repo.

## URL forms accepted

```
https://github.com/owner/repo            https://github.com/owner/repo.git
owner/repo                               git@github.com:owner/repo.git
https://github.com/owner/repo/tree/dev   ssh://git@host:2222/owner/repo
https://gitlab.com/group/sub/repo/-/tree/main
https://dev.azure.com/org/project/_git/repo
/path/to/a/local/repo                    file:///path/to/a/local/repo
https://github.com/myorg                 https://github.com/orgs/myorg   (org: pick repos)
```

A browser URL works as-is: the `/tree/<branch>`, `/blob/...`, `?query` and `#fragment`
parts are trimmed, and a branch found in the URL is used unless the branch field says
otherwise.

## Tokens

Private repos need a PAT (GitHub: *Settings → Developer settings → tokens*, `repo`
scope). It can come from the token field, `--token`, or
`$GITCLONER_PAT` / `$GITHUB_TOKEN` / `$GH_TOKEN` / `$GITLAB_TOKEN`.

The token is handed to git through `GIT_ASKPASS` — this binary answers git's
credential prompt from an environment variable — so it never appears in:

- the command line (so not in `ps`, `tasklist` or your shell history),
- `.git/config` as part of a remote URL,
- the log pane or an error message (it is replaced with `***` on the way out).

The username is picked per host when you leave it blank (`x-access-token` for GitHub,
`oauth2` for GitLab, `x-token-auth` for Bitbucket). SSH and local clones ignore the
token entirely and use your keys / filesystem.

## Edge cases handled

**Input**
- Paste a list in any shape: newlines, commas, semicolons or spaces.
- Duplicate URLs collapse (case-insensitive); the count says how many were dropped.
- Two repos with the same name get `repo` and `repo-2`, not one folder fighting over.
- Unparseable entries are listed with the reason and skipped — the rest still clone.
- `owner/repo` shorthand, trailing `.git`, trailing `/`, mixed case hosts.
- Credentials typed into a URL (`https://token@host/...`) are pulled out and used, not
  written to disk.
- Folder names are sanitised for Windows: `:`, `?`, `*`, trailing dots, and device
  names like `nul`.

**Destination**
- Missing parent directories are created; `~` expands.
- Existing non-empty folder → **skip** (default), **rename** (`repo-2`), or
  **overwrite**, which asks for confirmation before deleting anything.
- Overwrite refuses drive roots and anything the current directory sits inside.
- Existing *empty* folder is cloned into; a *file* or *symlink* in the way is reported.

**Auth and network**
- Private repo with no token fails immediately instead of hanging: terminal prompts are
  disabled (`GIT_TERMINAL_PROMPT=0`), stdin is closed, and the OS credential manager is
  bypassed when we supply a token.
- SSH runs with `BatchMode=yes`, so an unknown host key or a passphrase fails fast
  rather than blocking forever.
- Errors are translated into something actionable: PAT rejected vs. repo not found vs.
  needs a PAT vs. DNS/TLS/proxy vs. branch missing vs. transfer broke part-way.
- `LC_ALL=C` keeps git's messages in English so that classification works on any
  machine.

**While running**
- Up to N clones at once (default 4, adjustable live); the rest queue.
- Cancel one or all; the whole git process tree is killed, including helpers.
- A clone that fails or is cancelled takes its half-written folder with it — but only
  if we were the ones who created it.
- Retry individual repos or everything that failed, without re-typing anything.
- Exit code is 1 if anything failed or was rejected, so it is scriptable.

**Terminal**
- Panic or crash restores the terminal (no wrecked shell).
- Too-small window shows a message instead of a broken layout; resizing is fine.
- Long URLs and long lists scroll; non-ASCII text edits correctly.
- Windows: `core.longpaths=true`, key-release events ignored, process tree killed with
  `taskkill`.

## Releasing

Nobody bumps versions by hand. Every push to `main` runs the tests, then
[semantic-release](https://github.com/semantic-release/semantic-release) reads the
commit messages since the last `vX.Y.Z` tag and picks the next version:

| commit message starts with | release |
|---|---|
| `fix:` | patch — 0.2.0 → 0.2.1 |
| `feat:` | minor — 0.2.0 → 0.3.0 |
| `feat!:`, or a `BREAKING CHANGE:` line in the body | major — 0.2.0 → 1.0.0 |
| `docs:`, `chore:`, `refactor:`, `test:`, anything else | none |

It writes the version into `Cargo.toml` and `Cargo.lock`, commits that as
`chore(release): X.Y.Z`, tags it, and starts the **Release** workflow, which builds
every platform with [dist](https://github.com/axodotdev/cargo-dist) and publishes the
GitHub Release with the installers. Config: [.releaserc.json](.releaserc.json) and
[.github/workflows/semantic-release.yml](.github/workflows/semantic-release.yml).

If a release build fails, start it again from Actions → Release → Run workflow: pick
the tag under "Use workflow from" and type the same tag into the box.

## Tests

```
cargo test
```

Covers URL parsing, progress parsing, error classification, token redaction, folder
collisions, and a real end-to-end clone: it builds a git repo in a temp directory, then
clones it through the same code path the UI uses, including the skip / rename /
overwrite branches and the failure cleanup. No network needed.
