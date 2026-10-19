# Graph Report - gitcloner-cli  (2026-10-19)

## Corpus Check
- Corpus is ~35,011 words - fits in a single context window. You may not need a graph.

## Summary
- 468 nodes · 1092 edges · 17 communities (15 shown, 2 thin omitted)
- Extraction: 98% EXTRACTED · 2% INFERRED · 0% AMBIGUOUS · INFERRED: 19 edges (avg confidence: 0.84)
- Token cost: 176,287 input · 0 output

## Community Hubs (Navigation)
- App Event Loop
- TUI Rendering
- Clone Runner & Jobs
- Release Pipeline & Web Form
- URL Parsing
- Settings Persistence
- GitHub Org API
- Release Jobs & Brand Assets
- Text Input Editing
- Pixel Planet Art
- CLI Entry & Askpass
- Form Fields
- Job Status States
- Start Screen & Popups
- Screen Navigation
- Terminal Demo
- Project Root

## God Nodes (most connected - your core abstractions)
1. `App` - 72 edges
2. `th()` - 28 edges
3. `fg()` - 26 edges
4. `run_job()` - 21 edges
5. `Input` - 16 edges
6. `Modal` - 16 edges
7. `Event` - 15 edges
8. `Status` - 14 edges
9. `picker()` - 14 edges
10. `profile_panel()` - 14 edges

## Surprising Connections (you probably didn't know these)
- `compute()` --semantically_similar_to--> `Input splitting and duplicate collapse`  [INFERRED] [semantically similar]
  docs/index.html → README.md
- `Releasing process` --semantically_similar_to--> `Conventional-commit versioning (fix/feat/feat!)`  [INFERRED] [semantically similar]
  README.md → .github/workflows/semantic-release.yml
- `bashScript()` --semantically_similar_to--> `Fail-fast auth (GIT_TERMINAL_PROMPT=0, SSH BatchMode)`  [INFERRED] [semantically similar]
  docs/index.html → README.md
- `listOrg()` --semantically_similar_to--> `Org / user repo picker`  [INFERRED] [semantically similar]
  docs/index.html → README.md
- `parse()` --semantically_similar_to--> `Accepted URL forms`  [INFERRED] [semantically similar]
  docs/index.html → README.md

## Import Cycles
- 1-file cycle: `src/clone.rs -> src/clone.rs`
- 1-file cycle: `src/org.rs -> src/org.rs`
- 2-file cycle: `src/app.rs -> src/ui.rs -> src/app.rs`
- 2-file cycle: `src/settings.rs -> src/ui.rs -> src/settings.rs`
- 3-file cycle: `src/app.rs -> src/settings.rs -> src/ui.rs -> src/app.rs`

## Hyperedges (group relationships)
- **Push-to-release pipeline (semantic-release -> dist -> GitHub Release)** — _github_workflows_semantic_release_release, _github_workflows_release_plan, _github_workflows_release_build_local_artifacts, _github_workflows_release_build_global_artifacts, _github_workflows_release_host, readme_installer_scripts [EXTRACTED 1.00]
- **Browser form output generators** — docs_index_appcommand, docs_index_bashscript, docs_index_psscript, docs_index_asjson, docs_index_zipurl [EXTRACTED 1.00]
- **Token safety measures** — readme_git_askpass, readme_token_redaction, readme_fail_fast_auth, readme_token_env_vars, readme_per_host_username [INFERRED 0.85]

## Communities (17 total, 2 thin omitted)

### Community 0 - "App Event Loop"
Cohesion: 0.06
Nodes (39): a_failed_or_abandoned_org_listing_goes_back_to_the_form(), a_finished_batch_pops_up_a_summary(), App, branch_from_url_is_used_but_the_field_wins(), duplicates_collapse_and_name_clashes_get_their_own_folder(), empty_url_list_is_refused(), env_token(), Event (+31 more)

### Community 1 - "TUI Rendering"
Cohesion: 0.09
Nodes (61): ago(), bar(), BYLINE, comet(), dark(), days(), done_panel(), draw() (+53 more)

### Community 2 - "Clone Runner & Jobs"
Cohesion: 0.05
Nodes (47): JobView, a_cancelled_clone_reports_cancelled(), build_command(), classify(), clones_a_real_repo_and_handles_the_awkward_cases(), Dest, Skip, Use (+39 more)

### Community 3 - "Release Pipeline & Web Form"
Cohesion: 0.06
Nodes (30): cargo-dist (dist) v0.32.0, Release Workflow (dist), release job (version, commit, tag, dispatch), Semantic Release Workflow, test job (cargo test --locked), asJson(), bashScript(), Clone from here (browser form) (+22 more)

### Community 4 - "URL Parsing"
Cohesion: 0.09
Nodes (26): browser_urls_give_up_their_branch(), clean(), credentials_never_reach_the_url(), https_forms(), is_scp(), is_shorthand(), Kind, Https (+18 more)

### Community 5 - "Settings Persistence"
Cohesion: 0.09
Nodes (13): file(), ROWS, Settings, settings_survive_a_restart(), CURRENT, install_command(), latest(), LATEST_URL (+5 more)

### Community 6 - "GitHub Org API"
Cohesion: 0.15
Nodes (18): fake_org(), fake_repo(), Picker, Account, API, check(), fetch_all(), get() (+10 more)

### Community 7 - "Release Jobs & Brand Assets"
Cohesion: 0.12
Nodes (16): announce job, build-global-artifacts job, build-local-artifacts job, dist-manifest.json, host job (create GitHub Release), plan job, gitcloner landing page, gitcloner Wordmark (two-tone monospace) (+8 more)

### Community 8 - "Text Input Editing"
Cohesion: 0.25
Nodes (3): editing_is_char_safe(), Input, TextArea

### Community 9 - "Pixel Planet Art"
Cohesion: 0.20
Nodes (9): BAYER, DUST, fbm(), KeepOut, noise(), SEA, sky(), STAR (+1 more)

### Community 10 - "CLI Entry & Askpass"
Cohesion: 0.19
Nodes (5): Prefill, askpass(), HELP, main(), parse_args()

### Community 11 - "Form Fields"
Cohesion: 0.18
Nodes (11): Field, Branch, Dest, Existing, Parallel, Shallow, Submodules, Token (+3 more)

### Community 12 - "Job Status States"
Cohesion: 0.22
Nodes (8): Status, Cancelled, Cloned, Failed, Invalid, Pending, Running, Skipped

### Community 13 - "Start Screen & Popups"
Cohesion: 0.33
Nodes (5): draw(), planet() pixel-planet renderer, End-of-batch summary popup, settings.json (theme, animations, popup, update check), Start screen (pixel planet menu)

### Community 14 - "Screen Navigation"
Cohesion: 0.40
Nodes (5): Screen, Form, Pick, Progress, Splash

## Ambiguous Edges - Review These
- `GitHub REST API` → `listOrg()`  [AMBIGUOUS]
  docs/index.html · relation: calls

## Knowledge Gaps
- **50 isolated node(s):** `gitcloner`, `Splash`, `Form`, `Pick`, `Progress` (+45 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 148 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **2 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `App` connect `App Event Loop` to `TUI Rendering`, `Clone Runner & Jobs`, `Settings Persistence`, `GitHub Org API`, `Text Input Editing`, `Form Fields`, `Screen Navigation`?**
  _High betweenness centrality (0.134) - this node is a cross-community bridge._
- **What connects `gitcloner`, `Splash`, `Form` to the rest of the system?**
  _50 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `App Event Loop` be split into smaller, more focused modules?**
  _Cohesion score 0.058678611422172455 - nodes in this community are weakly interconnected._
- **Why does `OnExisting` connect `Clone Runner & Jobs` to `App Event Loop`, `TUI Rendering`?**
  _High betweenness centrality (0.034) - this node is a cross-community bridge._
- **Should `TUI Rendering` be split into smaller, more focused modules?**
  _Cohesion score 0.08611863896416742 - nodes in this community are weakly interconnected._
- **Why does `Field` connect `Form Fields` to `App Event Loop`, `TUI Rendering`?**
  _High betweenness centrality (0.033) - this node is a cross-community bridge._

### Low-confidence Hints
_AMBIGUOUS edges — the extractor was unsure. Verify before acting on these._

- **What is the exact relationship between `GitHub REST API` and `listOrg()`?**
  _Edge tagged AMBIGUOUS (relation: calls) - confidence is low._