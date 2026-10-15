# gitcloner

A terminal UI ([ratatui](https://ratatui.rs/)) for cloning a pile of git repos in one go —
public or private, several at a time, with a PAT only when one is needed.

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
 gitcloner  clone many repos — public or private · git version 2.51.0
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
 shallow: off  submodules: off  if folder exists: skip  at once: 4
```

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

## Cloning from an org

Put an org (or user) URL in the list — `https://github.com/myorg` or
`https://github.com/orgs/myorg/repositories` — and press **ctrl+s**. gitcloner lists
its repos and opens a tick list:

```
 gitcloner  myorg — 37 of 42 ticked · plus 1 other URL(s)
╭──────────────────────────────────────────────────────────────────────╮
│▸[x] myorg/api          private            the public API             │
│ [ ] myorg/fork-of-x    fork                                          │
│ [ ] myorg/old-site     archived           2019 marketing site        │
│ [x] myorg/web                             the web app                │
╰──────────────────────────────────────────────────────────────────────╯
 ↑↓ move · space tick · a all/none · enter clone ticked · esc back · q quit
```

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

## Tests

```
cargo test
```

Covers URL parsing, progress parsing, error classification, token redaction, folder
collisions, and a real end-to-end clone: it builds a git repo in a temp directory, then
clones it through the same code path the UI uses, including the skip / rename /
overwrite branches and the failure cleanup. No network needed.
