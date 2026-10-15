//! State, keys and the scheduler that keeps N clones in flight.

use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};

use crate::clone::{self, Handle, Job, OnExisting, Outcome, Update};
use crate::org::{self, OrgRepo};
use crate::repo::{self, Kind};
use crate::ui;

const LOG_LINES: usize = 300;

// ------------------------------------------------------------------ text input

/// One line of editable text. Cursor is a char index, so non-ASCII is safe.
#[derive(Default, Clone)]
pub struct Input {
    pub text: String,
    pub cursor: usize,
}

impl Input {
    pub fn new(s: &str) -> Self {
        Self { text: s.to_string(), cursor: s.chars().count() }
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_at(&self, idx: usize) -> usize {
        self.text.char_indices().nth(idx).map_or(self.text.len(), |(b, _)| b)
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
        }
    }

    /// Everything from the cursor to the end, removed.
    pub fn split_off(&mut self) -> String {
        let at = self.byte_at(self.cursor);
        self.text.split_off(at)
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') if ctrl => {
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                while self.cursor > 0 && self.text.chars().nth(self.cursor - 1) == Some(' ') {
                    self.backspace();
                }
                while self.cursor > 0 && self.text.chars().nth(self.cursor - 1) != Some(' ') {
                    self.backspace();
                }
            }
            KeyCode::Char(c) if !ctrl => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            _ => return false,
        }
        true
    }
}

/// The repo list: several `Input` lines with a row cursor.
#[derive(Default)]
pub struct TextArea {
    pub lines: Vec<Input>,
    pub row: usize,
}

impl TextArea {
    pub fn new(lines: &[String]) -> Self {
        let lines: Vec<Input> = lines.iter().map(|l| Input::new(l)).collect();
        let row = lines.len().saturating_sub(1);
        Self { lines: if lines.is_empty() { vec![Input::default()] } else { lines }, row }
    }

    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    fn cur(&mut self) -> &mut Input {
        if self.lines.is_empty() {
            self.lines.push(Input::default());
        }
        self.row = self.row.min(self.lines.len() - 1);
        &mut self.lines[self.row]
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.lines.get(self.row).map_or(0, |l| l.cursor))
    }

    fn newline(&mut self) {
        let rest = self.cur().split_off();
        self.row += 1;
        self.lines.insert(self.row, Input::new(&rest));
        self.lines[self.row].cursor = 0;
    }

    pub fn insert_str(&mut self, s: &str) {
        for (i, chunk) in s.split(['\n', '\r']).enumerate() {
            if i > 0 {
                self.newline();
            }
            self.cur().insert_str(chunk);
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => self.newline(),
            KeyCode::Up if self.row > 0 => {
                self.row -= 1;
                let max = self.lines[self.row].len();
                self.lines[self.row].cursor = self.lines[self.row].cursor.min(max);
            }
            KeyCode::Down if self.row + 1 < self.lines.len() => {
                self.row += 1;
                let max = self.lines[self.row].len();
                self.lines[self.row].cursor = self.lines[self.row].cursor.min(max);
            }
            KeyCode::Backspace if self.cur().cursor == 0 && self.row > 0 => {
                let line = self.lines.remove(self.row);
                self.row -= 1;
                let end = self.lines[self.row].len();
                self.lines[self.row].cursor = end;
                self.lines[self.row].text.push_str(&line.text);
            }
            _ => return self.cur().key(key),
        }
        true
    }
}

// ----------------------------------------------------------------- app pieces

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Form,
    Pick,
    Progress,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Urls,
    Dest,
    Branch,
    User,
    Token,
    Shallow,
    Submodules,
    Existing,
    Parallel,
}

const FIELDS: [Field; 9] = [
    Field::Urls,
    Field::Dest,
    Field::Branch,
    Field::User,
    Field::Token,
    Field::Shallow,
    Field::Submodules,
    Field::Existing,
    Field::Parallel,
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    ConfirmOverwrite(usize),
    ConfirmQuit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Pending,
    Running,
    Cloned,
    Failed,
    Skipped,
    Cancelled,
    Invalid,
}

impl Status {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    pub fn retryable(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::Skipped)
    }
}

pub struct JobView {
    pub label: String,
    pub status: Status,
    pub pct: u16,
    pub phase: String,
    pub detail: String,
    /// Latest `1.00 MiB | 2.00 MiB/s` from git; empty until data flows.
    pub transfer: String,
    pub log: VecDeque<String>,
    pub handle: Handle,
    pub started: Option<Instant>,
    pub elapsed: Option<Duration>,
    pub job: Option<Job>,
}

impl JobView {
    fn pending(job: Job) -> Self {
        Self {
            label: job.spec.name.clone(),
            status: Status::Pending,
            pct: 0,
            phase: "queued".into(),
            detail: job.dest.display().to_string(),
            transfer: String::new(),
            log: VecDeque::new(),
            handle: Handle::default(),
            started: None,
            elapsed: None,
            job: Some(job),
        }
    }

    fn invalid(entry: String, why: String) -> Self {
        Self {
            label: entry,
            status: Status::Invalid,
            pct: 0,
            phase: "rejected".into(),
            detail: why,
            transfer: String::new(),
            log: VecDeque::new(),
            handle: Handle::default(),
            started: None,
            elapsed: None,
            job: None,
        }
    }

    pub fn url(&self) -> &str {
        self.job.as_ref().map_or(self.label.as_str(), |j| j.spec.url.as_str())
    }

    pub fn secs(&self) -> Option<f32> {
        self.elapsed
            .or_else(|| self.started.map(|s| s.elapsed()))
            .map(|d| d.as_secs_f32())
    }
}

/// The org picker: repos listed from one or more org URLs, with a tick each.
#[derive(Default)]
pub struct Picker {
    /// The org names, for the title.
    pub orgs: String,
    pub loading: bool,
    pub repos: Vec<OrgRepo>,
    pub picked: Vec<bool>,
    pub row: usize,
    /// Plain URLs from the form, cloned alongside whatever is ticked.
    pub rest: Vec<String>,
}

pub enum Event {
    Term(TermEvent),
    Job(usize, Update),
    Done(usize, Outcome),
    /// An org listing finished; the id says which request it answers.
    Listed(u64, Result<Vec<OrgRepo>, String>),
    Tick,
}

#[derive(Default)]
pub struct Prefill {
    pub urls: Vec<String>,
    pub dest: Option<String>,
    pub user: Option<String>,
    pub token: Option<String>,
    pub branch: Option<String>,
    pub shallow: bool,
}

pub struct App {
    pub screen: Screen,
    pub field: Field,
    pub urls: TextArea,
    pub dest: Input,
    pub branch: Input,
    pub user: Input,
    pub token: Input,
    pub reveal: bool,
    pub shallow: bool,
    pub submodules: bool,
    pub existing: OnExisting,
    pub parallel: usize,
    pub banner: Option<(String, bool)>,
    pub jobs: Vec<JobView>,
    pub sel: usize,
    pub modal: Option<Modal>,
    /// Ticks since launch; every animation is a function of it.
    pub spinner: usize,
    /// The overall percentage as drawn: chases `overall()` so the bar glides.
    pub eased: f32,
    /// When the current list appeared, so its rows can cascade in.
    pub entered: Option<Instant>,
    pub git: String,
    pub pick: Picker,
    fetch_id: u64,
    list_repos: fn(&str, Option<&str>) -> Result<Vec<OrgRepo>, String>,
    quit: bool,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

fn env_token() -> Option<String> {
    ["GITCLONER_PAT", "GITHUB_TOKEN", "GH_TOKEN", "GITLAB_TOKEN"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn expand_home(path: &str) -> PathBuf {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
    match (path.strip_prefix('~'), home) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(path),
    }
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

impl App {
    pub fn new(prefill: Prefill, git: String) -> Self {
        let (tx, rx) = mpsc::channel();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let dest = prefill.dest.unwrap_or_else(|| cwd.display().to_string());
        let token = prefill.token.or_else(env_token);
        let has_urls = !prefill.urls.is_empty();
        Self {
            screen: Screen::Form,
            field: if has_urls { Field::Dest } else { Field::Urls },
            urls: TextArea::new(&prefill.urls),
            dest: Input::new(&dest),
            branch: Input::new(prefill.branch.as_deref().unwrap_or("")),
            user: Input::new(prefill.user.as_deref().unwrap_or("")),
            token: Input::new(token.as_deref().unwrap_or("")),
            reveal: false,
            shallow: prefill.shallow,
            submodules: false,
            existing: OnExisting::Skip,
            parallel: 4,
            banner: None,
            jobs: Vec::new(),
            sel: 0,
            modal: None,
            spinner: 0,
            eased: 0.0,
            entered: None,
            git,
            pick: Picker::default(),
            fetch_id: 0,
            list_repos: org::list,
            quit: false,
            tx,
            rx,
        }
    }

    pub fn run(mut self, term: &mut DefaultTerminal) -> io::Result<u8> {
        let keys = self.tx.clone();
        thread::spawn(move || {
            while let Ok(ev) = event::read() {
                if keys.send(Event::Term(ev)).is_err() {
                    break;
                }
            }
        });
        let ticks = self.tx.clone();
        thread::spawn(move || {
            while ticks.send(Event::Tick).is_ok() {
                thread::sleep(Duration::from_millis(80));
            }
        });

        term.draw(|f| ui::draw(f, &self))?;
        while !self.quit {
            let Ok(ev) = self.rx.recv() else { break };
            // Every screen animates, so every event repaints. ratatui only
            // writes the cells that changed, which keeps an idle frame cheap.
            self.handle(ev);
            while let Ok(ev) = self.rx.try_recv() {
                self.handle(ev);
            }
            term.draw(|f| ui::draw(f, &self))?;
        }
        // Killed clones tidy up the folders they were half-way through
        // writing; give those threads a moment to finish before we exit.
        self.cancel_all();
        let deadline = Instant::now() + Duration::from_millis(1500);
        while self.busy() && Instant::now() < deadline {
            if let Ok(ev) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.handle(ev);
            }
        }
        Ok(u8::from(self.jobs.iter().any(|j| matches!(j.status, Status::Failed | Status::Invalid))))
    }

    // ------------------------------------------------------------ event plumbing

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Tick => {
                self.spinner = self.spinner.wrapping_add(1);
                let target = self.overall();
                self.eased += (target - self.eased) * 0.3;
                if (target - self.eased).abs() < 0.3 {
                    self.eased = target;
                }
            }
            Event::Term(TermEvent::Key(key)) if key.kind == KeyEventKind::Press => self.key(key),
            Event::Term(TermEvent::Paste(text)) => self.paste(&text),
            Event::Term(_) => {}
            Event::Job(idx, update) => {
                if let Some(job) = self.jobs.get_mut(idx) {
                    match update {
                        Update::Phase(p) => job.phase = p,
                        Update::Progress(p) => job.pct = job.pct.max(p),
                        Update::Transfer(t) => job.transfer = t,
                        Update::Log(line) => {
                            if job.log.len() >= LOG_LINES {
                                job.log.pop_front();
                            }
                            job.log.push_back(line);
                        }
                    }
                }
            }
            Event::Done(idx, outcome) => {
                if let Some(job) = self.jobs.get_mut(idx) {
                    job.elapsed = job.started.map(|s| s.elapsed());
                    match outcome {
                        Outcome::Cloned(path) => {
                            job.status = Status::Cloned;
                            job.pct = 100;
                            job.phase = "done".into();
                            job.detail = path.display().to_string();
                        }
                        Outcome::Skipped(why) => {
                            job.status = Status::Skipped;
                            job.phase = "skipped".into();
                            job.detail = why;
                        }
                        Outcome::Failed(why) => {
                            job.status = Status::Failed;
                            job.phase = "failed".into();
                            job.detail = why.clone();
                            if job.log.len() >= LOG_LINES {
                                job.log.pop_front();
                            }
                            job.log.push_back(why);
                        }
                        Outcome::Cancelled => {
                            job.status = Status::Cancelled;
                            job.phase = "cancelled".into();
                            job.detail = "cancelled".into();
                        }
                    }
                }
                self.pump();
            }
            Event::Listed(id, result) => {
                if id != self.fetch_id || !self.pick.loading {
                    return; // the user went back before it arrived
                }
                self.pick.loading = false;
                let error = match result {
                    Ok(repos) if repos.is_empty() => Some(format!(
                        "{}: no repositories you can see — add a PAT for private ones",
                        self.pick.orgs
                    )),
                    Ok(repos) => {
                        self.pick.picked = repos.iter().map(|r| !r.fork && !r.archived).collect();
                        self.pick.repos = repos;
                        self.entered = Some(Instant::now());
                        None
                    }
                    Err(why) => Some(why),
                };
                if let Some(why) = error {
                    self.screen = Screen::Form;
                    self.field = Field::Urls;
                    self.banner = Some((why, true));
                }
            }
        }
    }

    fn paste(&mut self, text: &str) {
        match self.field {
            Field::Urls => self.urls.insert_str(text),
            _ => {
                let flat: String = text.chars().filter(|c| !c.is_control()).collect();
                if let Some(input) = self.text_field() {
                    input.insert_str(flat.trim());
                }
            }
        }
    }

    fn text_field(&mut self) -> Option<&mut Input> {
        match self.field {
            Field::Dest => Some(&mut self.dest),
            Field::Branch => Some(&mut self.branch),
            Field::User => Some(&mut self.user),
            Field::Token => Some(&mut self.token),
            _ => None,
        }
    }

    fn key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.request_quit();
            return;
        }
        if let Some(modal) = self.modal {
            self.modal_key(modal, key);
            return;
        }
        if ctrl && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) {
            if self.screen == Screen::Form {
                self.start();
            }
            return;
        }
        if ctrl && matches!(key.code, KeyCode::Char('t') | KeyCode::Char('T')) {
            self.reveal = !self.reveal;
            return;
        }
        match self.screen {
            Screen::Form => self.form_key(key),
            Screen::Pick => self.pick_key(key),
            Screen::Progress => self.progress_key(key),
        }
    }

    fn modal_key(&mut self, modal: Modal, key: KeyEvent) {
        let yes = matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter);
        let no = matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc);
        if !yes && !no {
            return;
        }
        self.modal = None;
        match (modal, yes) {
            (Modal::ConfirmQuit, true) => self.quit = true,
            (Modal::ConfirmOverwrite(_), true) => self.pump(),
            (Modal::ConfirmOverwrite(_), false) => {
                self.screen = Screen::Form;
                self.field = Field::Existing;
                self.jobs.clear();
                self.banner = Some(("nothing was deleted".into(), false));
            }
            _ => {}
        }
    }

    fn form_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => self.cycle_field(1),
            KeyCode::BackTab => self.cycle_field(-1),
            KeyCode::F(5) => self.start(),
            KeyCode::Esc => self.request_quit(),
            _ => {}
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::F(5) | KeyCode::Esc) {
            return;
        }
        match self.field {
            Field::Urls => {
                self.urls.key(key);
            }
            Field::Dest | Field::Branch | Field::User | Field::Token => {
                if key.code == KeyCode::Enter {
                    self.start();
                } else if key.code == KeyCode::Down {
                    self.cycle_field(1);
                } else if key.code == KeyCode::Up {
                    self.cycle_field(-1);
                } else if let Some(input) = self.text_field() {
                    input.key(key);
                }
            }
            Field::Shallow | Field::Submodules | Field::Existing | Field::Parallel => {
                self.toggle_key(key)
            }
        }
    }

    fn toggle_key(&mut self, key: KeyEvent) {
        let forward = matches!(
            key.code,
            KeyCode::Right | KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Char('+')
        );
        let back = matches!(key.code, KeyCode::Left | KeyCode::Char('-'));
        match key.code {
            KeyCode::Up => return self.cycle_field(-1),
            KeyCode::Down => return self.cycle_field(1),
            KeyCode::Char(c) if c.is_ascii_digit() && self.field == Field::Parallel => {
                let n = c.to_digit(10).unwrap_or(4) as usize;
                self.parallel = n.clamp(1, 16);
                return;
            }
            _ => {}
        }
        if !forward && !back {
            return;
        }
        match self.field {
            Field::Shallow => self.shallow = !self.shallow,
            Field::Submodules => self.submodules = !self.submodules,
            Field::Existing => {
                self.existing = if forward { self.existing.next() } else { self.existing.prev() }
            }
            Field::Parallel => {
                let step = if forward { 1 } else { -1 };
                self.parallel = (self.parallel as i32 + step).clamp(1, 16) as usize;
            }
            _ => {}
        }
    }

    fn cycle_field(&mut self, delta: i32) {
        let at = FIELDS.iter().position(|f| *f == self.field).unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(FIELDS.len() as i32) as usize;
        self.field = FIELDS[next];
    }

    fn pick_key(&mut self, key: KeyEvent) {
        let p = &mut self.pick;
        let last = p.repos.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('b') | KeyCode::Left => {
                p.loading = false;
                self.screen = Screen::Form;
                self.banner = None;
            }
            KeyCode::Char('q') => self.request_quit(),
            _ if p.loading => {}
            KeyCode::Down | KeyCode::Char('j') => p.row = (p.row + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => p.row = p.row.saturating_sub(1),
            KeyCode::Home => p.row = 0,
            KeyCode::End => p.row = last,
            KeyCode::Char(' ') => {
                if let Some(on) = p.picked.get_mut(p.row) {
                    *on = !*on;
                }
            }
            KeyCode::Char('a') => {
                let all = p.picked.iter().all(|on| *on);
                p.picked.fill(!all);
            }
            KeyCode::Enter => self.confirm_pick(),
            _ => {}
        }
    }

    fn progress_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.jobs.is_empty() {
                    self.sel = (self.sel + 1).min(self.jobs.len() - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.sel = self.sel.saturating_sub(1),
            KeyCode::Home => self.sel = 0,
            KeyCode::End => self.sel = self.jobs.len().saturating_sub(1),
            KeyCode::Char('c') => self.cancel(self.sel),
            KeyCode::Char('C') => self.cancel_all(),
            KeyCode::Char('r') => self.retry(self.sel),
            KeyCode::Char('R') => {
                for idx in 0..self.jobs.len() {
                    self.retry(idx);
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.parallel = (self.parallel + 1).min(16);
                self.pump();
            }
            KeyCode::Char('-') => self.parallel = self.parallel.saturating_sub(1).max(1),
            KeyCode::Esc | KeyCode::Char('b') | KeyCode::Left => {
                if self.busy() {
                    self.banner =
                        Some(("still cloning — press C to cancel everything first".into(), true));
                } else {
                    self.screen = Screen::Form;
                    self.banner = None;
                }
            }
            KeyCode::Char('q') => self.request_quit(),
            _ => {}
        }
    }

    // ----------------------------------------------------------------- actions

    pub fn busy(&self) -> bool {
        self.jobs.iter().any(|j| j.status.is_active())
    }

    /// 0..=100 across every job; finished ones count as complete.
    pub fn overall(&self) -> f32 {
        if self.jobs.is_empty() {
            return 0.0;
        }
        let sum: f32 = self
            .jobs
            .iter()
            .map(|j| if j.status.is_active() { f32::from(j.pct) } else { 100.0 })
            .sum();
        sum / self.jobs.len() as f32
    }

    pub fn count(&self, status: Status) -> usize {
        self.jobs.iter().filter(|j| j.status == status).count()
    }

    fn request_quit(&mut self) {
        if self.busy() {
            self.modal = Some(Modal::ConfirmQuit);
        } else {
            self.quit = true;
        }
    }

    fn dest_root(&self) -> PathBuf {
        let text = self.dest.text.trim();
        if text.is_empty() {
            return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        }
        expand_home(text)
    }

    fn start(&mut self) {
        self.banner = None;
        let entries = repo::split_inputs(&self.urls.text());
        if entries.is_empty() {
            self.field = Field::Urls;
            self.banner = Some(("add at least one repository URL".into(), true));
            return;
        }
        let mut orgs: Vec<String> = Vec::new();
        let mut rest = Vec::new();
        for entry in entries {
            match repo::parse_org(&entry) {
                Some(name) if orgs.iter().any(|o| o.eq_ignore_ascii_case(&name)) => {}
                Some(name) => orgs.push(name),
                None => rest.push(entry),
            }
        }
        if orgs.is_empty() {
            return self.queue(rest);
        }

        // An org means "some of its repos": list them off-thread, then let
        // the user tick the ones they want.
        self.fetch_id += 1;
        let id = self.fetch_id;
        self.pick = Picker { orgs: orgs.join(", "), loading: true, rest, ..Picker::default() };
        self.screen = Screen::Pick;
        let (tx, list, token) = (self.tx.clone(), self.list_repos, non_empty(&self.token.text));
        thread::spawn(move || {
            let result = orgs.iter().try_fold(Vec::new(), |mut all, name| {
                all.extend(list(name, token.as_deref()).map_err(|e| format!("{name}: {e}"))?);
                Ok::<_, String>(all)
            });
            let _ = tx.send(Event::Listed(id, result));
        });
    }

    fn confirm_pick(&mut self) {
        let p = &self.pick;
        let mut entries = p.rest.clone();
        let ticked = p.repos.iter().zip(&p.picked).filter(|(_, on)| **on);
        entries.extend(ticked.map(|(r, _)| r.url.clone()));
        if entries.is_empty() {
            self.banner = Some(("nothing ticked — space ticks a repo, a ticks them all".into(), true));
            return;
        }
        self.queue(entries);
    }

    /// Turn entries into jobs and start the progress screen.
    fn queue(&mut self, entries: Vec<String>) {
        let root = self.dest_root();
        let branch = non_empty(&self.branch.text);
        let user = non_empty(&self.user.text);
        let token = non_empty(&self.token.text);

        let mut jobs: Vec<JobView> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut names: HashSet<String> = HashSet::new();
        let mut dupes = 0usize;
        let mut first_error: Option<String> = None;

        for entry in entries {
            match repo::parse(&entry) {
                Err(why) => {
                    first_error.get_or_insert_with(|| format!("{entry}: {why}"));
                    jobs.push(JobView::invalid(entry, why));
                }
                Ok(spec) => {
                    if !seen.insert(spec.url.to_ascii_lowercase()) {
                        dupes += 1;
                        continue;
                    }
                    // Two different repos can share a name: give the second one
                    // its own folder instead of colliding.
                    let mut name = spec.name.clone();
                    let mut n = 2;
                    while !names.insert(name.to_ascii_lowercase()) {
                        name = format!("{}-{n}", spec.name);
                        n += 1;
                    }
                    let token = match spec.kind {
                        Kind::Https => spec.token.clone().or_else(|| token.clone()),
                        _ => None, // SSH and local clones do not use a PAT
                    };
                    jobs.push(JobView::pending(Job {
                        dest: root.join(&name),
                        branch: branch.clone().or_else(|| spec.branch.clone()),
                        shallow: self.shallow,
                        submodules: self.submodules,
                        user: user.clone().or_else(|| spec.user.clone()),
                        token,
                        on_existing: self.existing,
                        spec,
                    }));
                }
            }
        }

        let valid = jobs.iter().filter(|j| j.status == Status::Pending).count();
        if valid == 0 {
            self.screen = Screen::Form;
            self.field = Field::Urls;
            self.banner = Some((
                first_error.unwrap_or_else(|| "nothing to clone".into()),
                true,
            ));
            return;
        }
        let bad = jobs.len() - valid;
        let mut note = format!("{valid} queued");
        if dupes > 0 {
            note += &format!(" · {dupes} duplicate(s) dropped");
        }
        if bad > 0 {
            note += &format!(" · {bad} unusable");
        }
        self.banner = Some((note, bad > 0));
        self.jobs = jobs;
        self.sel = 0;
        self.eased = 0.0;
        self.entered = Some(Instant::now());
        self.screen = Screen::Progress;

        let doomed = self
            .jobs
            .iter()
            .filter_map(|j| j.job.as_ref())
            .filter(|j| {
                j.on_existing == OnExisting::Overwrite
                    && std::fs::read_dir(&j.dest).is_ok_and(|mut d| d.next().is_some())
            })
            .count();
        if doomed > 0 {
            self.modal = Some(Modal::ConfirmOverwrite(doomed));
        }
        self.pump();
    }

    /// Start pending jobs until `parallel` are in flight.
    fn pump(&mut self) {
        if self.modal.is_some() {
            return;
        }
        let mut free = self
            .parallel
            .saturating_sub(self.count(Status::Running));
        for idx in 0..self.jobs.len() {
            if free == 0 {
                break;
            }
            if self.jobs[idx].status == Status::Pending {
                self.spawn(idx);
                free -= 1;
            }
        }
    }

    fn spawn(&mut self, idx: usize) {
        let Some(job) = self.jobs[idx].job.clone() else { return };
        let handle = Handle::default();
        let view = &mut self.jobs[idx];
        view.handle = handle.clone();
        view.status = Status::Running;
        view.started = Some(Instant::now());
        view.elapsed = None;
        view.pct = 0;
        view.phase = "starting".into();
        view.detail = job.dest.display().to_string();
        view.transfer.clear();
        view.log.clear();

        let updates = self.tx.clone();
        let done = self.tx.clone();
        thread::spawn(move || {
            let mut emit = |u: Update| {
                let _ = updates.send(Event::Job(idx, u));
            };
            let outcome = clone::run_job(&job, &handle, &mut emit);
            let _ = done.send(Event::Done(idx, outcome));
        });
    }

    fn cancel(&mut self, idx: usize) {
        let Some(view) = self.jobs.get_mut(idx) else { return };
        match view.status {
            Status::Running => {
                view.phase = "cancelling".into();
                view.handle.cancel();
            }
            Status::Pending => {
                view.status = Status::Cancelled;
                view.phase = "cancelled".into();
                view.detail = "never started".into();
            }
            _ => {}
        }
    }

    fn cancel_all(&mut self) {
        for idx in 0..self.jobs.len() {
            self.cancel(idx);
        }
    }

    fn retry(&mut self, idx: usize) {
        let Some(view) = self.jobs.get_mut(idx) else { return };
        if !view.status.retryable() || view.job.is_none() {
            return;
        }
        view.status = Status::Pending;
        view.phase = "queued".into();
        view.pct = 0;
        view.elapsed = None;
        view.started = None;
        view.transfer.clear();
        view.log.clear();
        self.pump();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parallel = 0` means `pump` starts nothing, so a test that only cares
    /// about the queue never shells out to git.
    fn app() -> App {
        let mut a = App::new(Prefill::default(), "git version test".into());
        a.parallel = 0;
        a
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn editing_is_char_safe() {
        let mut input = Input::new("héllo");
        input.cursor = 2;
        input.backspace();
        assert_eq!(input.text, "hllo");
        input.insert('é');
        assert_eq!((input.text.as_str(), input.cursor), ("héllo", 2));
        input.key(press(KeyCode::End));
        input.key(press(KeyCode::Backspace));
        assert_eq!(input.text, "héll");
    }

    #[test]
    fn textarea_splits_and_joins_lines() {
        let mut area = TextArea::default();
        area.insert_str("a/b\nc/d");
        assert_eq!(area.text(), "a/b\nc/d");
        area.key(press(KeyCode::Up));
        area.key(press(KeyCode::End));
        area.key(press(KeyCode::Enter));
        area.insert_str("x/y");
        assert_eq!(area.text(), "a/b\nx/y\nc/d");
        // joining lines back up with backspace at column 0
        area.key(press(KeyCode::Home));
        area.key(press(KeyCode::Backspace));
        assert_eq!(area.text(), "a/bx/y\nc/d");
    }

    #[test]
    fn empty_url_list_is_refused() {
        let mut a = app();
        a.start();
        assert!(a.banner.as_ref().unwrap().1, "should be an error banner");
        assert!(a.jobs.is_empty());
        assert_eq!(a.screen as u8, Screen::Form as u8);
    }

    #[test]
    fn duplicates_collapse_and_name_clashes_get_their_own_folder() {
        let mut a = app();
        a.dest = Input::new("out");
        a.urls.insert_str(
            "https://github.com/one/repo\nhttps://github.com/ONE/repo\nhttps://gitlab.com/two/repo\nnonsense",
        );
        a.start();
        let dests: Vec<String> = a
            .jobs
            .iter()
            .filter_map(|j| j.job.as_ref())
            .map(|j| j.dest.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(dests, ["repo", "repo-2"]);
        assert_eq!(a.count(Status::Invalid), 1);
        let note = a.banner.as_ref().unwrap();
        assert!(note.0.contains("duplicate") && note.0.contains("unusable"), "{}", note.0);
    }

    #[test]
    fn branch_from_url_is_used_but_the_field_wins() {
        let mut a = app();
        a.urls.insert_str("https://github.com/o/r/tree/dev");
        a.start();
        assert_eq!(a.jobs[0].job.as_ref().unwrap().branch.as_deref(), Some("dev"));

        let mut a = app();
        a.branch = Input::new("main");
        a.urls.insert_str("https://github.com/o/r/tree/dev");
        a.start();
        assert_eq!(a.jobs[0].job.as_ref().unwrap().branch.as_deref(), Some("main"));
    }

    #[test]
    fn ssh_jobs_never_carry_the_pat() {
        let mut a = app();
        a.token = Input::new("ghp_token");
        a.urls.insert_str("git@github.com:o/r.git\nhttps://github.com/o/r2");
        a.start();
        let jobs: Vec<&Job> = a.jobs.iter().filter_map(|j| j.job.as_ref()).collect();
        assert_eq!(jobs[0].token, None);
        assert_eq!(jobs[1].token.as_deref(), Some("ghp_token"));
    }

    /// The real thing: three repos, two workers, through the same scheduler,
    /// threads and channel the UI uses. Local repos, so no network.
    #[test]
    fn the_scheduler_clones_several_repos_at_once() {
        use crate::clone::testing::{make_repo, temp_root};

        let root = temp_root("app");
        let names = ["alpha", "beta", "gamma"];
        for name in names {
            make_repo(&root.join(name));
        }
        let out = root.join("out");

        let mut a = app();
        a.parallel = 2;
        a.dest = Input::new(&out.display().to_string());
        for name in names {
            a.urls.insert_str(&root.join(name).display().to_string());
            a.urls.key(press(KeyCode::Enter));
        }
        a.urls.insert_str("https://example.invalid/not/a/real/repo?x");
        a.start();
        assert_eq!(a.count(Status::Pending) + a.count(Status::Running), 4);
        assert!(a.count(Status::Running) <= 2, "more workers than asked for");

        // drain the channel exactly like run() does
        let deadline = Instant::now() + Duration::from_secs(60);
        while a.busy() && Instant::now() < deadline {
            if let Ok(ev) = a.rx.recv_timeout(Duration::from_millis(200)) {
                a.handle(ev);
            }
        }
        assert!(!a.busy(), "clones never finished");
        assert_eq!(a.count(Status::Cloned), 3, "{:#?}", a.jobs.iter().map(|j| &j.detail).collect::<Vec<_>>());
        assert_eq!(a.count(Status::Failed), 1, "the unreachable host should fail");
        for name in names {
            assert!(out.join(name).join("README.md").exists(), "{name} not cloned");
        }
        let failed = a.jobs.iter().find(|j| j.status == Status::Failed).unwrap();
        assert!(!failed.detail.is_empty() && failed.pct < 100);

        // retry puts a finished job back in the queue and runs it again
        a.existing = OnExisting::Skip;
        a.jobs[0].job.as_mut().unwrap().on_existing = OnExisting::Skip;
        a.jobs[0].status = Status::Failed;
        a.retry(0);
        while a.busy() && Instant::now() < deadline {
            if let Ok(ev) = a.rx.recv_timeout(Duration::from_millis(200)) {
                a.handle(ev);
            }
        }
        assert_eq!(a.jobs[0].status, Status::Skipped, "{}", a.jobs[0].detail);

        let _ = std::fs::remove_dir_all(&root);
    }

    fn fake_org(name: &str, _token: Option<&str>) -> Result<Vec<OrgRepo>, String> {
        let repo = |n: &str, fork: bool, archived: bool| OrgRepo {
            full_name: format!("{name}/{n}"),
            url: format!("https://github.com/{name}/{n}.git"),
            description: format!("the {n} repo"),
            private: n == "api",
            fork,
            archived,
            language: if n == "web" { "TypeScript".into() } else { "Rust".into() },
            stars: 1204,
            forks: 87,
            issues: 23,
            size_kb: 12_700,
            pushed: "2026-09-30".into(),
            branch: "main".into(),
            license: "MIT".into(),
            topics: vec!["cli".into(), "git".into()],
        };
        Ok(vec![
            repo("api", false, false),
            repo("forked", true, false),
            repo("old", false, true),
            repo("web", false, false),
        ])
    }

    fn next_event(a: &App) -> Event {
        a.rx.recv_timeout(Duration::from_secs(5)).expect("the org listing never arrived")
    }

    /// An org URL opens a tick list; only what is ticked gets queued, next to
    /// the plain URLs. The listing is faked, so no network.
    #[test]
    fn org_urls_open_a_picker_and_queue_only_ticked_repos() {
        let mut a = app();
        a.list_repos = fake_org;
        a.urls.insert_str(
            "https://github.com/myorg
https://github.com/o/plain
https://github.com/orgs/MYORG/repositories",
        );
        a.start();
        assert!(a.screen == Screen::Pick && a.pick.loading);
        assert_eq!(a.pick.rest, ["https://github.com/o/plain"]);

        let ev = next_event(&a);
        a.handle(ev);
        assert!(!a.pick.loading);
        assert_eq!(a.pick.repos.len(), 4, "the same org twice is listed once");
        assert_eq!(a.pick.picked, [true, false, false, true], "forks and archived start unticked");

        // nothing ticked and no plain URLs: refuse, stay put
        let rest = std::mem::take(&mut a.pick.rest);
        a.key(press(KeyCode::Char('a')));
        a.key(press(KeyCode::Char('a')));
        assert_eq!(a.pick.picked, [false; 4], "a twice: all, then none");
        a.key(press(KeyCode::Enter));
        assert!(a.screen == Screen::Pick && a.banner.as_ref().unwrap().1);
        a.pick.rest = rest;

        a.key(press(KeyCode::Char(' '))); // api
        a.key(press(KeyCode::Down));
        a.key(press(KeyCode::Char(' '))); // forked
        a.key(press(KeyCode::Enter));
        assert!(a.screen == Screen::Progress);
        let urls: Vec<&str> = a.jobs.iter().map(|j| j.url()).collect();
        assert_eq!(
            urls,
            ["https://github.com/o/plain", "https://github.com/myorg/api", "https://github.com/myorg/forked"]
        );
    }

    #[test]
    fn a_failed_or_abandoned_org_listing_goes_back_to_the_form() {
        let mut a = app();
        a.list_repos = |_, _| Err("no GitHub org or user by that name".into());
        a.urls.insert_str("https://github.com/nope");

        // esc while it loads: the late answer is ignored
        a.start();
        a.key(press(KeyCode::Esc));
        assert!(a.screen == Screen::Form);
        let ev = next_event(&a);
        a.handle(ev);
        assert!(a.banner.is_none(), "a stale listing should be dropped");

        a.start();
        let ev = next_event(&a);
        a.handle(ev);
        assert!(a.screen == Screen::Form);
        let (msg, is_error) = a.banner.clone().unwrap();
        assert!(is_error && msg.contains("nope: no GitHub org"), "{msg}");
    }

    /// Draws every screen at awkward sizes. Layout arithmetic is where a TUI
    /// panics, so this is the check that it never does.
    /// `cargo test render -- --nocapture` prints the screens.
    #[test]
    fn every_screen_renders_at_every_size() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut a = app();
        a.parallel = 0;
        a.token = Input::new("ghp_secret_token_value");
        a.urls.insert_str(
            "https://github.com/owner/repo\nhttps://gitlab.com/group/sub/other\nnot-a-url",
        );
        a.start();
        // one of each status, with text long enough to need truncating
        let statuses = [
            Status::Cloned,
            Status::Running,
            Status::Failed,
            Status::Skipped,
            Status::Cancelled,
        ];
        for (job, status) in a.jobs.iter_mut().zip(statuses) {
            job.status = status;
            job.pct = 42;
            job.started = Some(Instant::now());
            job.detail = "a very long explanation ".repeat(8);
            job.transfer = "12.30 MiB | 5.00 MiB/s".into();
            job.log.push_back("remote: Counting objects: 100% (12/12), done.".into());
        }

        a.pick.orgs = "a-very-long-organisation-name-that-goes-on".into();
        a.pick.rest = vec!["owner/repo".into()];
        a.pick.repos = fake_org("an-org-with-a-long-name", None).unwrap();
        a.pick.picked = vec![true, false, true, false];
        a.pick.row = 3;
        a.entered = None; // skip the cascade-in, so every row is drawn

        for (w, h) in [(100, 30), (60, 15), (200, 60), (40, 10), (61, 16), (300, 16)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            for loading in [true, false] {
                a.pick.loading = loading;
                a.screen = Screen::Pick;
                a.modal = None;
                term.draw(|f| ui::draw(f, &a)).unwrap();
            }
            if (w, h) == (100, 30) {
                println!("{}", term.backend());
            }
            for screen in [Screen::Form, Screen::Progress] {
                a.screen = screen;
                for field in FIELDS {
                    a.field = field;
                    a.modal = None;
                    term.draw(|f| ui::draw(f, &a)).unwrap();
                }
                for modal in [Modal::ConfirmQuit, Modal::ConfirmOverwrite(3)] {
                    a.modal = Some(modal);
                    term.draw(|f| ui::draw(f, &a)).unwrap();
                }
            }
            if (w, h) == (100, 30) {
                a.screen = Screen::Progress;
                a.modal = None;
                term.draw(|f| ui::draw(f, &a)).unwrap();
                println!("{}", term.backend());
                let dump = term.backend().to_string();
                assert!(!dump.contains("ghp_secret"), "token rendered on the progress screen");
            }
        }

        // the form must never print the token in the clear unless asked
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        a.screen = Screen::Form;
        a.modal = None;
        a.reveal = false;
        term.draw(|f| ui::draw(f, &a)).unwrap();
        println!("{}", term.backend());
        assert!(!term.backend().to_string().contains("ghp_secret"), "token shown unmasked");
        a.reveal = true;
        term.draw(|f| ui::draw(f, &a)).unwrap();
        assert!(term.backend().to_string().contains("ghp_secret"), "ctrl+t should reveal it");
    }

    #[test]
    fn queued_jobs_cancel_and_retry_without_touching_git() {
        let mut a = app();
        a.parallel = 0; // nothing gets spawned, so everything stays queued
        a.urls.insert_str("https://github.com/o/a\nhttps://github.com/o/b\nnot-a-url");
        a.start();
        assert_eq!(a.count(Status::Pending), 2);

        a.cancel_all();
        assert_eq!(a.count(Status::Cancelled), 2);
        // a rejected entry is not something we can cancel or retry
        assert_eq!(a.count(Status::Invalid), 1);

        a.retry(0);
        assert_eq!(a.count(Status::Pending), 1);
        let invalid = a.jobs.iter().position(|j| j.status == Status::Invalid).unwrap();
        a.retry(invalid);
        assert_eq!(a.count(Status::Invalid), 1);
    }
}
