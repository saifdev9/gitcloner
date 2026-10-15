//! Listing the repos of a GitHub org or user, so the user can pick some.
//!
//! Talks to the GitHub API through `curl`: it ships with Windows 10+, Git for
//! Windows, macOS and Linux, and uses the same proxy and certificate setup git
//! does. The token goes to curl on stdin as config, never on its command line.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::repo;

const API: &str = "https://api.github.com";
const PER_PAGE: usize = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrgRepo {
    /// `owner/name`, for display.
    pub full_name: String,
    /// HTTPS clone URL.
    pub url: String,
    pub description: String,
    pub private: bool,
    pub fork: bool,
    pub archived: bool,
    pub language: String,
    pub stars: u64,
    pub forks: u64,
    pub issues: u64,
    /// What GitHub stores, in KiB — roughly what a full clone downloads.
    pub size_kb: u64,
    /// Last push, `YYYY-MM-DD`.
    pub pushed: String,
    pub branch: String,
    /// SPDX id, e.g. `MIT`; empty when there is none or GitHub cannot tell.
    pub license: String,
    pub topics: Vec<String>,
    /// GitHub's node id, as `gh repo view --json id` prints it (`R_kgDO…`).
    pub id: String,
    /// The older numeric id.
    pub db_id: u64,
}

/// The GitHub account a token belongs to, for the profile panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub login: String,
    pub name: String,
    pub bio: String,
    pub company: String,
    pub location: String,
    pub public_repos: u64,
    pub private_repos: u64,
    pub followers: u64,
    pub following: u64,
    /// Account created, `YYYY-MM-DD`.
    pub since: String,
    pub url: String,
}

pub fn account(token: &str) -> Result<Account, String> {
    let (code, body) = get(&format!("{API}/user"), Some(token))?;
    check(code, &body)?;
    parse_account(&body)
}

fn parse_account(body: &str) -> Result<Account, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("unexpected reply from GitHub: {e}"))?;
    let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let n = |k: &str| v[k].as_u64().unwrap_or(0);
    let login = s("login");
    if login.is_empty() {
        return Err("unexpected reply from GitHub: no login".into());
    }
    Ok(Account {
        login,
        name: s("name"),
        bio: s("bio"),
        company: s("company"),
        location: s("location"),
        public_repos: n("public_repos"),
        private_repos: n("owned_private_repos"),
        followers: n("followers"),
        following: n("following"),
        since: s("created_at").chars().take(10).collect(),
        url: s("html_url"),
    })
}

/// Every repo of `name` the token can see: as an org first, then as a user.
/// A user's private repos only show up through `/user/repos`, to their own token.
pub fn list(name: &str, token: Option<&str>) -> Result<Vec<OrgRepo>, String> {
    let token = token.filter(|t| !t.is_empty());
    if let Some(repos) = fetch_all(&format!("{API}/orgs/{name}/repos?type=all"), token)? {
        return Ok(repos);
    }
    let me = match token {
        Some(t) => Some(account(t)?.login),
        None => None,
    };
    let base = if me.is_some_and(|m| m.eq_ignore_ascii_case(name)) {
        format!("{API}/user/repos?affiliation=owner")
    } else {
        format!("{API}/users/{name}/repos?type=owner")
    };
    fetch_all(&base, token)?.ok_or_else(|| "no GitHub org or user by that name".to_string())
}

/// All pages of a listing; `None` when the account does not exist.
fn fetch_all(base: &str, token: Option<&str>) -> Result<Option<Vec<OrgRepo>>, String> {
    let mut repos = Vec::new();
    for page in 1.. {
        let url = format!("{base}&sort=full_name&per_page={PER_PAGE}&page={page}");
        let (code, body) = get(&url, token)?;
        if code == 404 && page == 1 {
            return Ok(None);
        }
        check(code, &body)?;
        let batch = parse_page(&body)?;
        let last = batch.len() < PER_PAGE;
        repos.extend(batch);
        if last {
            break;
        }
    }
    Ok(Some(repos))
}

/// One GET through curl. Returns the HTTP status and the body.
fn get(url: &str, token: Option<&str>) -> Result<(u16, String), String> {
    let mut child = Command::new("curl")
        .args(["--silent", "--show-error", "--location", "--max-time", "30"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .args(["--write-out", r"\n%{http_code}", "--config", "-", "--url", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "curl is not on PATH — it is needed to list an org's repos".to_string()
            }
            _ => format!("could not run curl: {e}"),
        })?;
    // Dropping stdin closes it, so curl stops reading config either way.
    if let (Some(mut stdin), Some(token)) = (child.stdin.take(), token) {
        let _ = writeln!(stdin, "header = \"Authorization: Bearer {}\"", quote(token));
    }
    let out = child.wait_with_output().map_err(|e| format!("curl failed: {e}"))?;
    if !out.status.success() {
        let err = repo::redact(String::from_utf8_lossy(&out.stderr).trim(), token);
        return Err(format!("could not reach GitHub — {err}"));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    let code = code.trim().parse().map_err(|_| "curl gave no HTTP status".to_string())?;
    Ok((code, body.to_string()))
}

/// A value inside double quotes in curl's config syntax.
fn quote(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Turn a non-2xx reply into one line a human can act on.
fn check(code: u16, body: &str) -> Result<(), String> {
    if (200..300).contains(&code) {
        return Ok(());
    }
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["message"].as_str().map(str::to_string))
        .unwrap_or_default();
    Err(match code {
        401 => "PAT rejected — check it has not expired".into(),
        403 | 429 if msg.to_ascii_lowercase().contains("rate limit") => {
            "GitHub API rate limit hit — add a PAT (60 requests an hour without one)".into()
        }
        _ if msg.is_empty() => format!("GitHub answered HTTP {code}"),
        _ => format!("GitHub answered HTTP {code}: {msg}"),
    })
}

fn parse_page(body: &str) -> Result<Vec<OrgRepo>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("unexpected reply from GitHub: {e}"))?;
    let items = v.as_array().ok_or("unexpected reply from GitHub: not a list")?;
    Ok(items.iter().map(parse_repo).collect())
}

/// One repo object, as GitHub's REST API describes it.
fn parse_repo(r: &Value) -> OrgRepo {
    let s = |k: &str| r[k].as_str().unwrap_or("").to_string();
    let b = |k: &str| r[k].as_bool().unwrap_or(false);
    let n = |k: &str| r[k].as_u64().unwrap_or(0);
    OrgRepo {
        full_name: s("full_name"),
        url: s("clone_url"),
        description: s("description"),
        private: b("private"),
        fork: b("fork"),
        archived: b("archived"),
        language: s("language"),
        stars: n("stargazers_count"),
        forks: n("forks_count"),
        issues: n("open_issues_count"),
        size_kb: n("size"),
        pushed: s("pushed_at").chars().take(10).collect(),
        branch: s("default_branch"),
        license: r["license"]["spdx_id"]
            .as_str()
            .filter(|id| *id != "NOASSERTION")
            .unwrap_or("")
            .to_string(),
        topics: r["topics"]
            .as_array()
            .map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        id: s("node_id"),
        db_id: n("id"),
    }
}

/// One repo, looked up by name — for repos pasted one by one.
pub fn repo(owner: &str, name: &str, token: Option<&str>) -> Result<OrgRepo, String> {
    let token = token.filter(|t| !t.is_empty());
    let (code, body) = get(&format!("{API}/repos/{owner}/{name}"), token)?;
    if code == 404 {
        return Err(if token.is_some() {
            "not found, or the PAT cannot see it".into()
        } else {
            "not found — private? add a PAT".into()
        });
    }
    check(code, &body)?;
    let v: Value =
        serde_json::from_str(&body).map_err(|e| format!("unexpected reply from GitHub: {e}"))?;
    Ok(parse_repo(&v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_github_listing() {
        let body = r#"[
            {"full_name": "acme/api", "clone_url": "https://github.com/acme/api.git",
             "description": "the API", "private": true, "fork": false, "archived": false,
             "language": "Rust", "stargazers_count": 1204, "forks_count": 87,
             "open_issues_count": 23, "size": 12700, "pushed_at": "2026-09-30T08:12:45Z",
             "default_branch": "main", "license": {"spdx_id": "MIT"}, "topics": ["cli", "git"],
             "id": 812345678, "node_id": "R_kgDOMG7mTg"},
            {"full_name": "acme/old", "clone_url": "https://github.com/acme/old.git",
             "description": null, "private": false, "fork": true, "archived": true,
             "language": null, "license": {"spdx_id": "NOASSERTION"}}
        ]"#;
        let repos = parse_page(body).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].url, "https://github.com/acme/api.git");
        assert!(repos[0].private && !repos[0].fork);
        assert_eq!((repos[0].stars, repos[0].forks, repos[0].issues, repos[0].size_kb), (1204, 87, 23, 12700));
        assert_eq!((repos[0].language.as_str(), repos[0].pushed.as_str()), ("Rust", "2026-09-30"));
        assert_eq!((repos[0].license.as_str(), repos[0].branch.as_str()), ("MIT", "main"));
        assert_eq!(repos[0].topics, ["cli", "git"]);
        assert_eq!((repos[0].id.as_str(), repos[0].db_id), ("R_kgDOMG7mTg", 812_345_678));
        assert_eq!(repos[1].description, "");
        assert_eq!((repos[1].language.as_str(), repos[1].license.as_str()), ("", ""));
        assert!(repos[1].fork && repos[1].archived);
        assert!(parse_page(r#"{"message": "Not Found"}"#).is_err());
    }

    #[test]
    fn reads_the_token_owner() {
        let body = r#"{"login": "saifdev9", "name": "Saif", "bio": null, "public_repos": 12,
            "owned_private_repos": 3, "followers": 40, "following": 7,
            "created_at": "2021-04-02T10:00:00Z", "html_url": "https://github.com/saifdev9"}"#;
        let a = parse_account(body).unwrap();
        assert_eq!((a.login.as_str(), a.name.as_str(), a.bio.as_str()), ("saifdev9", "Saif", ""));
        assert_eq!((a.public_repos, a.private_repos, a.followers), (12, 3, 40));
        assert_eq!(a.since, "2021-04-02");
        assert!(parse_account(r#"{"message": "Bad credentials"}"#).is_err());
    }

    #[test]
    fn api_errors_are_actionable() {
        assert!(check(200, "[]").is_ok());
        assert!(check(401, "").unwrap_err().contains("PAT rejected"));
        let limited = r#"{"message": "API rate limit exceeded for 1.2.3.4."}"#;
        assert!(check(403, limited).unwrap_err().contains("rate limit"));
        let sso = r#"{"message": "Resource protected by organization SAML enforcement."}"#;
        assert!(check(403, sso).unwrap_err().contains("SAML"));
        assert_eq!(check(500, "oops").unwrap_err(), "GitHub answered HTTP 500");
    }

    #[test]
    fn token_cannot_break_out_of_curl_config() {
        assert_eq!(quote("ghp_abc"), "ghp_abc");
        assert_eq!(quote("a\"b\\c\nurl = \"x\""), "a\\\"b\\\\curl = \\\"x\\\"");
    }

    /// Needs the network: `cargo test -- --ignored`.
    #[test]
    #[ignore = "hits api.github.com"]
    fn lists_a_real_account() {
        // octocat is a user, not an org: exercises the fallback too
        let repos = list("octocat", None).unwrap();
        assert!(repos.iter().any(|r| r.full_name == "octocat/Hello-World"), "{repos:#?}");
        // a real org with more than one page of repos
        let repos = list("rust-lang", None).unwrap();
        assert!(repos.len() > PER_PAGE, "only {} repos: paging stopped early", repos.len());
        assert!(repos.iter().any(|r| r.full_name == "rust-lang/rust"));
        // one pasted repo, looked up by name
        let hello = repo("octocat", "Hello-World", None).unwrap();
        assert_eq!(hello.full_name, "octocat/Hello-World");
        assert!(!hello.id.is_empty() && hello.db_id == 1_296_269, "{hello:#?}");
        assert!(repo("octocat", "no-such-repo-x9z", None).unwrap_err().contains("not found"));
        let err = list("gitcloner-no-such-account-x9z", None).unwrap_err();
        assert!(err.contains("no GitHub org or user"), "{err}");
    }
}
