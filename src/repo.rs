//! Turning whatever the user pasted into something `git clone` accepts.
//!
//! Accepts: `https://host/owner/repo(.git)`, browser URLs with `/tree/<branch>`,
//! GitLab subgroups and `/-/` paths, `git@host:owner/repo`, `ssh://`, `git://`,
//! `file://`, local paths, and the `owner/repo` shorthand (assumed GitHub).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Https,
    Ssh,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSpec {
    /// What the user typed, for display.
    pub input: String,
    /// What we hand to git — credentials are always stripped out of this.
    pub url: String,
    /// Directory name to clone into.
    pub name: String,
    /// Lowercased host (with port if given); empty for local paths.
    pub host: String,
    /// Branch picked out of a browser URL (`/tree/<branch>`).
    pub branch: Option<String>,
    /// Credentials found in the URL, removed from `url` so they never hit
    /// `.git/config`, the process table or a log line.
    pub token: Option<String>,
    pub user: Option<String>,
    pub kind: Kind,
}

/// Path segments that mean "this is a browser URL, the repo path stopped here".
const WEB_CUTS: &[&str] = &[
    "-", "tree", "blob", "commit", "commits", "compare", "releases", "tags", "issues", "pull",
    "pulls", "merge_requests", "actions", "wiki", "raw", "archive", "branches", "settings",
    "discussions",
];

/// Windows device names that cannot be directory names.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

const TRIM_CHARS: [char; 8] = ['"', '\'', '`', '<', '>', '(', ')', ','];

fn clean(s: &str) -> String {
    s.trim().trim_matches(|c| TRIM_CHARS.contains(&c)).trim().to_string()
}

/// Split a pasted blob into candidate entries: newline, comma or semicolon
/// separated. A line is only split on spaces when it is not an existing path,
/// so a local path with spaces in it survives.
pub fn split_inputs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split(['\n', '\r', ',', ';']) {
        let line = clean(line);
        if line.is_empty() {
            continue;
        }
        if line.contains(char::is_whitespace) && !Path::new(&line).exists() {
            out.extend(line.split_whitespace().map(clean).filter(|s| !s.is_empty()));
        } else {
            out.push(line);
        }
    }
    out
}

fn looks_like_token(s: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "glptt-",
        "gitlab-ci-token",
    ];
    PREFIXES.iter().any(|p| s.starts_with(p))
        || (s.len() >= 20 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
}

/// Default username to pair with a PAT, per host. Most hosts ignore the
/// username, GitLab and Bitbucket do not.
pub fn default_user(host: &str) -> &'static str {
    let h = host.split(':').next().unwrap_or(host);
    if h.contains("gitlab") {
        "oauth2"
    } else if h.contains("bitbucket") {
        "x-token-auth"
    } else {
        "x-access-token"
    }
}

fn sanitize_name(raw: &str) -> Result<String, String> {
    let bad = |c: char| {
        matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\') || c.is_control()
    };
    let mut name: String = raw.chars().map(|c| if bad(c) { '_' } else { c }).collect();
    name = name.trim().trim_end_matches(['.', ' ']).trim().to_string();
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("cannot work out a folder name from {raw:?}"));
    }
    let stem = name.split('.').next().unwrap_or(&name).to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        name.push('_');
    }
    Ok(name)
}

fn strip_git(s: &str) -> &str {
    s.strip_suffix(".git").unwrap_or(s)
}

fn name_from_path(path: &str) -> Result<String, String> {
    let last = path
        .split(['/', '\\'])
        .rfind(|s| !s.is_empty() && *s != ".")
        .ok_or_else(|| "no repository name in the URL".to_string())?;
    sanitize_name(strip_git(last))
}

/// Parse one entry. Errors are user-facing.
pub fn parse(input: &str) -> Result<RepoSpec, String> {
    let raw = clean(input);
    if raw.is_empty() {
        return Err("empty entry".into());
    }
    let lower = raw.to_ascii_lowercase();

    if lower.starts_with("http://") || lower.starts_with("https://") {
        return parse_url(&raw, Kind::Https);
    }
    if lower.starts_with("ssh://") || lower.starts_with("git+ssh://") {
        return parse_url(&raw, Kind::Ssh);
    }
    if lower.starts_with("git://") {
        return parse_url(&raw, Kind::Https);
    }
    if lower.starts_with("file://") {
        return parse_local(&raw);
    }
    if is_scp(&raw) {
        return parse_scp(&raw);
    }
    if Path::new(&raw).exists() {
        return parse_local(&raw);
    }
    if is_shorthand(&raw) {
        let mut spec = parse_url(&format!("https://github.com/{}", strip_git(&raw)), Kind::Https)?;
        spec.input = raw;
        return Ok(spec);
    }
    if lower.starts_with("www.") || lower.contains(".com/") || lower.contains(".org/") {
        return Err("missing scheme — try https://".into());
    }
    Err("not a git URL (expected https://host/owner/repo, git@host:owner/repo, owner/repo or a local path)".into())
}

/// A GitHub org or user page — `https://github.com/myorg`, `/orgs/myorg` —
/// which stands for "pick from its repos". Returns the account name.
pub fn parse_org(input: &str) -> Option<String> {
    let raw = clean(input);
    let (scheme, rest) = raw.split_once("://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return None;
    }
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let mut segs = rest.split('/').filter(|s| !s.is_empty());
    let host = segs.next()?.rsplit('@').next()?.to_ascii_lowercase();
    if host != "github.com" && host != "www.github.com" {
        return None;
    }
    let name = match segs.collect::<Vec<_>>()[..] {
        [name] => name,
        [orgs, name] | [orgs, name, _] if orgs.eq_ignore_ascii_case("orgs") => name,
        _ => return None,
    };
    // GitHub account names are letters, digits and hyphens; anything else
    // is not an account, and must not reach the API URL built from it.
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        .then(|| name.to_string())
}

/// `git@host:owner/repo.git` — scp syntax: no scheme, colon after the host.
fn is_scp(s: &str) -> bool {
    if s.contains("://") {
        return false;
    }
    match (s.find('@'), s.find(':')) {
        (Some(at), Some(colon)) => colon > at && !s[..colon].contains('/'),
        _ => false,
    }
}

fn is_shorthand(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && !p.contains(char::is_whitespace)
                && !p.contains(':')
                && !p.contains('\\')
                && *p != "."
                && *p != ".."
        })
        && !WEB_CUTS.contains(&parts[1].to_ascii_lowercase().as_str())
}

fn parse_url(raw: &str, kind: Kind) -> Result<RepoSpec, String> {
    let (scheme, rest) = raw.split_once("://").ok_or("missing scheme")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    if hostport.is_empty() {
        return Err("missing host in URL".into());
    }

    let (mut user, mut token) = (None, None);
    if let Some(ui) = userinfo.filter(|s| !s.is_empty()) {
        match ui.split_once(':') {
            Some((u, p)) => {
                if !u.is_empty() {
                    user = Some(u.to_string());
                }
                if !p.is_empty() {
                    token = Some(p.to_string());
                }
            }
            None if looks_like_token(ui) => token = Some(ui.to_string()),
            None => user = Some(ui.to_string()),
        }
    }

    // Drop query and fragment, then cut off browser path noise.
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut branch = None;
    let cut = segs
        .iter()
        .position(|s| WEB_CUTS.contains(&s.to_ascii_lowercase().as_str()))
        // Keep at least owner + repo before a cut word, so a repo that is
        // genuinely called "tree" still works.
        .filter(|p| *p >= 2);
    if let Some(pos) = cut {
        let after = &segs[pos..];
        let (marker, rest) = if after[0] == "-" && after.len() > 1 {
            (after[1], &after[2..])
        } else {
            (after[0], &after[1..])
        };
        let marker = marker.to_ascii_lowercase();
        let names_a_ref = matches!(marker.as_str(), "tree" | "blob" | "commits" | "branches");
        branch = rest
            .first()
            .map(|b| strip_git(b))
            .filter(|b| names_a_ref && !b.is_empty())
            .map(str::to_string);
        segs.truncate(pos);
    }
    if let Some(last) = segs.last_mut() {
        *last = strip_git(last);
        if last.is_empty() {
            segs.pop();
        }
    }
    if segs.is_empty() {
        return Err("URL has no repository path".into());
    }

    let name = name_from_path(&segs.join("/"))?;
    let scheme = scheme.to_ascii_lowercase();
    let host = hostport.to_ascii_lowercase();
    let url = if kind == Kind::Ssh {
        // SSH keeps its username (git@...); there is no token to leak there.
        let prefix = user.as_deref().map(|u| format!("{u}@")).unwrap_or_default();
        token = None;
        format!("{scheme}://{prefix}{host}/{}", segs.join("/"))
    } else {
        format!("{scheme}://{host}/{}", segs.join("/"))
    };
    Ok(RepoSpec { input: raw.to_string(), url, name, host, branch, token, user, kind })
}

fn parse_scp(raw: &str) -> Result<RepoSpec, String> {
    let (userhost, path) = raw.split_once(':').ok_or("expected host:path")?;
    if path.trim().is_empty() {
        return Err("no repository path after ':'".into());
    }
    let host = userhost.rsplit('@').next().unwrap_or(userhost).to_ascii_lowercase();
    let user = userhost.split('@').next().filter(|u| !u.is_empty()).map(str::to_string);
    Ok(RepoSpec {
        input: raw.to_string(),
        url: raw.to_string(),
        name: name_from_path(path)?,
        host,
        branch: None,
        token: None,
        user,
        kind: Kind::Ssh,
    })
}

fn parse_local(raw: &str) -> Result<RepoSpec, String> {
    let stripped = raw.strip_prefix("file://").unwrap_or(raw);
    // file:///C:/repo -> C:/repo
    let stripped = match stripped.strip_prefix('/') {
        Some(rest) if rest.len() > 1 && rest.as_bytes()[1] == b':' => rest,
        _ => stripped,
    };
    let path = PathBuf::from(stripped);
    if !path.exists() {
        return Err(format!("local path does not exist: {stripped}"));
    }
    Ok(RepoSpec {
        input: raw.to_string(),
        url: stripped.to_string(),
        name: name_from_path(stripped)?,
        host: String::new(),
        branch: None,
        token: None,
        user: None,
        kind: Kind::Local,
    })
}

/// Hide a token (and any `user:pass@` inside a URL) before anything is shown.
pub fn redact(text: &str, token: Option<&str>) -> String {
    let mut out = text.to_string();
    if let Some(t) = token.filter(|t| t.len() >= 6) {
        out = out.replace(t, "***");
    }
    if !out.contains("://") || !out.contains('@') {
        return out;
    }
    out.split(' ')
        .map(|word| match word.split_once("://") {
            Some((scheme, rest)) => match rest.split_once('@') {
                Some((_, tail)) => format!("{scheme}://***@{tail}"),
                None => word.to_string(),
            },
            None => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> RepoSpec {
        parse(s).unwrap_or_else(|e| panic!("{s:?} failed: {e}"))
    }

    #[test]
    fn https_forms() {
        for s in [
            "https://github.com/owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo/",
            "  https://github.com/owner/repo.git/  ",
            "https://GitHub.com/owner/repo",
            "https://github.com/owner/repo?tab=readme#top",
        ] {
            let spec = p(s);
            assert_eq!(spec.name, "repo", "{s}");
            assert_eq!(spec.url, "https://github.com/owner/repo", "{s}");
            assert_eq!(spec.kind, Kind::Https);
        }
    }

    #[test]
    fn browser_urls_give_up_their_branch() {
        let spec = p("https://github.com/owner/repo/tree/feature");
        assert_eq!((spec.name.as_str(), spec.branch.as_deref()), ("repo", Some("feature")));
        let spec = p("https://gitlab.com/group/sub/repo/-/tree/dev");
        assert_eq!(spec.url, "https://gitlab.com/group/sub/repo");
        assert_eq!((spec.name.as_str(), spec.branch.as_deref()), ("repo", Some("dev")));
        let spec = p("https://github.com/owner/repo/blob/main/src/main.rs");
        assert_eq!((spec.name.as_str(), spec.branch.as_deref()), ("repo", Some("main")));
        // a repo actually called "tree" must survive
        assert_eq!(p("https://github.com/owner/tree").name, "tree");
    }

    #[test]
    fn credentials_never_reach_the_url() {
        let spec = p("https://ghp_0123456789abcdefghij@github.com/owner/repo.git");
        assert_eq!(spec.url, "https://github.com/owner/repo");
        assert_eq!(spec.token.as_deref(), Some("ghp_0123456789abcdefghij"));
        let spec = p("https://alice:s3cret-token@git.example.com:8443/team/repo");
        assert_eq!(spec.url, "https://git.example.com:8443/team/repo");
        assert_eq!(spec.user.as_deref(), Some("alice"));
        assert_eq!(spec.token.as_deref(), Some("s3cret-token"));
    }

    #[test]
    fn ssh_and_shorthand() {
        let spec = p("git@github.com:owner/repo.git");
        assert_eq!(
            (spec.kind, spec.name.as_str(), spec.host.as_str()),
            (Kind::Ssh, "repo", "github.com")
        );
        assert_eq!(spec.url, "git@github.com:owner/repo.git");
        let spec = p("ssh://git@ssh.dev.azure.com:22/org/proj/repo");
        assert_eq!((spec.kind, spec.name.as_str()), (Kind::Ssh, "repo"));
        assert_eq!(spec.url, "ssh://git@ssh.dev.azure.com:22/org/proj/repo");
        let spec = p("owner/repo");
        assert_eq!(spec.url, "https://github.com/owner/repo");
        assert_eq!(spec.input, "owner/repo");
    }

    #[test]
    fn deep_paths_keep_the_last_segment() {
        assert_eq!(p("https://dev.azure.com/org/project/_git/repo").name, "repo");
        assert_eq!(p("https://gitlab.com/a/b/c/d/repo.git").name, "repo");
        assert_eq!(p("https://git.example.com/repo.git").name, "repo");
    }

    #[test]
    fn rejects_rubbish() {
        for s in ["", "   ", "not a url", "https://", "http:///owner/repo", "/", "repo"] {
            assert!(parse(s).is_err(), "{s:?} should be rejected");
        }
    }

    #[test]
    fn splitting_handles_paste_shapes() {
        let text = "https://github.com/a/b\n  https://github.com/c/d , e/f\n\n; g/h i/j";
        assert_eq!(
            split_inputs(text),
            ["https://github.com/a/b", "https://github.com/c/d", "e/f", "g/h", "i/j"]
        );
    }

    #[test]
    fn windows_hostile_names() {
        assert_eq!(sanitize_name("nul").unwrap(), "nul_");
        assert_eq!(sanitize_name("repo.").unwrap(), "repo");
        assert_eq!(sanitize_name("we:ird|name").unwrap(), "we_ird_name");
        assert!(sanitize_name("..").is_err());
    }

    #[test]
    fn redaction() {
        assert_eq!(
            redact("fatal: bad token ghp_secret_value", Some("ghp_secret_value")),
            "fatal: bad token ***"
        );
        assert_eq!(
            redact("remote: https://user:pw@github.com/a/b failed", None),
            "remote: https://***@github.com/a/b failed"
        );
    }

    #[test]
    fn org_urls() {
        for (s, name) in [
            ("https://github.com/myorg", "myorg"),
            ("https://github.com/myorg/", "myorg"),
            ("https://www.GitHub.com/My-Org?tab=repositories", "My-Org"),
            ("https://github.com/orgs/myorg", "myorg"),
            ("https://github.com/orgs/myorg/repositories", "myorg"),
        ] {
            assert_eq!(parse_org(s).as_deref(), Some(name), "{s}");
        }
        for s in [
            "https://github.com/owner/repo",
            "https://github.com/owner/repo/tree/main",
            "https://git.example.com/repo.git",
            "https://gitlab.com/group",
            "owner/repo",
            "myorg",
            "https://github.com/",
            "https://github.com/my.org",
            "https://github.com/orgs/x/../../evil",
        ] {
            assert_eq!(parse_org(s), None, "{s} is not an org");
        }
    }

    #[test]
    fn host_default_users() {
        assert_eq!(default_user("gitlab.com"), "oauth2");
        assert_eq!(default_user("bitbucket.org"), "x-token-auth");
        assert_eq!(default_user("github.com"), "x-access-token");
    }
}
