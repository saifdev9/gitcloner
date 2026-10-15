//! Everything that draws. No state is changed here.
//!
//! Every animation is a pure function of `tick(app)` (one tick per 80 ms)
//! and the clock, so any frame can be drawn at any moment.

use std::cell::Cell;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use crate::app::{App, Field, Input, Modal, Screen, Status};
use crate::clone::OnExisting;
use crate::{settings, update};

/// One accent; every other colour means a result. 256-colour indexes rather
/// than truecolor, so a theme looks the same everywhere, macOS Terminal.app
/// included.
#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub about: &'static str,
    accent: Color,
    accent_hi: Color,
    accent_lo: Color,
    ok: Color,
    fail: Color,
    warn: Color,
    text: Color,
    muted: Color,
    dim: Color,
    faint: Color,
    /// Text on an accent background.
    ink: Color,
}

/// A theme for dark terminals: everything but the accent is shared.
const fn dark(name: &'static str, about: &'static str, accent: u8, hi: u8, lo: u8) -> Theme {
    Theme {
        name,
        about,
        accent: Color::Indexed(accent),
        accent_hi: Color::Indexed(hi),
        accent_lo: Color::Indexed(lo),
        ok: Color::Indexed(114),
        fail: Color::Indexed(167),
        warn: Color::Indexed(179),
        text: Color::Reset,
        muted: Color::Indexed(247),
        dim: Color::Indexed(243),
        faint: Color::Indexed(238),
        ink: Color::Indexed(234),
    }
}

pub const THEMES: [Theme; 6] = [
    dark("ocean", "calm blue", 75, 153, 25),
    dark("mint", "green-teal", 79, 158, 29),
    dark("ember", "warm orange", 173, 223, 130),
    dark("rose", "deep rose", 168, 218, 89),
    dark("mono", "greys only", 252, 255, 240),
    Theme {
        name: "paper",
        about: "for light terminals",
        accent: Color::Indexed(25),
        accent_hi: Color::Indexed(75),
        accent_lo: Color::Indexed(153),
        ok: Color::Indexed(28),
        fail: Color::Indexed(124),
        warn: Color::Indexed(130),
        text: Color::Reset,
        muted: Color::Indexed(240),
        dim: Color::Indexed(244),
        faint: Color::Indexed(250),
        ink: Color::Indexed(231),
    },
];

thread_local! {
    /// The theme of the frame being drawn; `draw` sets it before anything else.
    static THEME: Cell<Theme> = const { Cell::new(THEMES[0]) };
}

fn th() -> Theme {
    THEME.with(Cell::get)
}

/// The animation clock, frozen at 0 when animations are off.
fn tick(app: &App) -> usize {
    if app.settings.motion { app.spinner } else { 0 }
}

/// Language dots: the same language always gets the same colour.
const LANG: [Color; 6] = [
    Color::Indexed(173),
    Color::Indexed(110),
    Color::Indexed(150),
    Color::Indexed(180),
    Color::Indexed(174),
    Color::Indexed(116),
];

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const PULSE: [&str; 4] = ["∙", "•", "●", "•"];
const TAGLINE: &str = "clone many repos — public or private";
const BYLINE: &str =
    concat!("made by ", env!("CARGO_PKG_AUTHORS"), " · v", env!("CARGO_PKG_VERSION"));
const MIN_W: u16 = 60;
const MIN_H: u16 = 15;

pub fn draw(f: &mut Frame, app: &App) {
    THEME.with(|t| t.set(THEMES[app.settings.theme.min(THEMES.len() - 1)]));
    let area = f.area();
    if area.width < MIN_W || area.height < MIN_H {
        let msg = format!(
            "window is {}x{} — gitcloner needs at least {MIN_W}x{MIN_H}",
            area.width, area.height
        );
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }).fg(th().warn).centered(), area);
        return;
    }
    match app.screen {
        Screen::Form => form(f, area, app),
        Screen::Pick => picker(f, area, app),
        Screen::Progress => progress(f, area, app),
    }
    match app.modal {
        Some(Modal::Help) => help_panel(f, area, app),
        Some(Modal::Settings) => settings_panel(f, area, app),
        Some(Modal::Profile) => profile_panel(f, area, app),
        Some(Modal::Done) => done_panel(f, area, app),
        Some(modal) => render_modal(f, area, app, modal),
        None => {}
    }
}

// --------------------------------------------------------------------- helpers

fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

fn fit(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let keep: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{keep}…")
}

fn pad(s: &str, width: usize) -> String {
    let s = fit(s, width);
    let count = s.chars().count();
    format!("{s}{}", " ".repeat(width.saturating_sub(count)))
}

/// `text` in `base`, with a three-cell band of `hi` passing through it once
/// every `period` ticks.
fn sweep(text: &str, tick: usize, period: usize, base: Style, hi: Style) -> Vec<Span<'static>> {
    let at = (tick % period) as isize - 2;
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            let lit = (i as isize - at).abs() <= 1;
            Span::styled(c.to_string(), if lit { hi } else { base })
        })
        .collect()
}

/// A thin bar in half-cell steps. `glint` is a running count that walks a
/// highlight along the filled part, so a bar that is working visibly moves.
fn bar(frac: f32, width: usize, color: Color, glint: Option<usize>) -> Vec<Span<'static>> {
    let halves = (frac.clamp(0.0, 1.0) * (width * 2) as f32).round() as usize;
    let (full, half) = (halves / 2, halves % 2 == 1);
    let mut spans = Vec::new();
    match glint.map(|g| g % (full + 10)).filter(|g| *g < full) {
        Some(g) => {
            spans.push(Span::styled("━".repeat(g), fg(color)));
            spans.push(Span::styled("━", fg(th().accent_hi)));
            spans.push(Span::styled("━".repeat(full - g - 1), fg(color)));
        }
        None => spans.push(Span::styled("━".repeat(full), fg(color))),
    }
    if half {
        spans.push(Span::styled("╸", fg(color)));
    }
    let used = full + usize::from(half);
    spans.push(Span::styled("─".repeat(width.saturating_sub(used)), fg(th().faint)));
    spans
}

/// No percentage yet (connecting, counting): a comet runs along the track.
fn comet(tick: usize, width: usize) -> Vec<Span<'static>> {
    let head = tick % (width + 4);
    (0..width)
        .map(|i| match head.checked_sub(i) {
            Some(0) => Span::styled("━", fg(th().accent_hi)),
            Some(1) => Span::styled("━", fg(th().accent)),
            Some(2 | 3) => Span::styled("━", fg(th().accent_lo)),
            _ => Span::styled("─", fg(th().faint)),
        })
        .collect()
}

/// Brand badge with a glint passing through it, a breathing status dot,
/// whatever the screen has to say, and the byline on the right.
fn header(f: &mut Frame, area: Rect, app: &App, dot: Color, live: bool, rest: Vec<Span<'static>>) {
    let badge = fg(th().ink).bg(th().accent).bold();
    let mut spans = sweep(" gitcloner ", tick(app), 50, badge, fg(th().ink).bg(th().accent_hi).bold());
    let pulse = if live && app.settings.motion { PULSE[tick(app) / 3 % PULSE.len()] } else { "●" };
    spans.push(Span::styled(format!(" {pulse} "), fg(dot)));
    spans.extend(rest);

    let by = Line::from(Span::styled(format!("  {BYLINE} "), fg(th().dim))).right_aligned();
    let show_by = area.width as usize >= by.width() + 40;
    let left = if show_by { Rect { width: area.width - by.width() as u16, ..area } } else { area };
    f.render_widget(Line::from(spans), left);
    if show_by {
        f.render_widget(by, area);
    }
}

fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (key, what) in pairs {
        spans.push(Span::styled(key.to_string(), fg(th().muted).bold()));
        spans.push(Span::styled(format!(" {what}   "), fg(th().dim)));
    }
    Line::from(spans)
}

/// Hints for this screen on the left, the panel keys on the right when
/// there is room for both.
fn footer(f: &mut Frame, area: Rect, keys: &[(&str, &str)]) {
    let left = hints(keys);
    let menu = hints(&[("F1", "help"), ("F2", "settings"), ("F3", "profile")]).right_aligned();
    let fits = left.width() + menu.width() <= area.width as usize;
    f.render_widget(left, area);
    if fits {
        f.render_widget(menu, area);
    }
}

/// Groups of spans with a quiet separator between the non-empty ones.
fn join(groups: Vec<Vec<Span<'static>>>) -> Line<'static> {
    let mut spans = Vec::new();
    for group in groups.into_iter().filter(|g| !g.is_empty()) {
        if !spans.is_empty() {
            spans.push(Span::styled("  ·  ", fg(th().dim)));
        }
        spans.extend(group);
    }
    Line::from(spans)
}

/// Rows of a list that has just appeared cascade in, one every 20 ms.
fn revealed(app: &App) -> usize {
    match app.entered {
        Some(t) if app.settings.motion => t.elapsed().as_millis() as usize / 20 + 1,
        _ => usize::MAX,
    }
}

fn panel<'a>() -> Block<'a> {
    Block::bordered().border_type(BorderType::Rounded).border_style(fg(th().faint))
}

fn field_block(title: &str, focused: bool) -> Block<'_> {
    if focused {
        panel().border_style(fg(th().accent)).title(Span::styled(title, fg(th().accent).bold()))
    } else {
        panel().title(Span::styled(title, fg(th().muted)))
    }
}

fn render_input(
    f: &mut Frame,
    area: Rect,
    title: &str,
    input: &Input,
    focused: bool,
    placeholder: &str,
    mask: bool,
) {
    let block = field_block(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 {
        return;
    }
    let shown: String = if mask { "•".repeat(input.len()) } else { input.text.clone() };
    let width = inner.width as usize;
    let scroll = input.cursor.saturating_sub(width.saturating_sub(1));
    let visible: String = shown.chars().skip(scroll).take(width).collect();
    let line = if visible.is_empty() && !placeholder.is_empty() {
        Line::from(Span::styled(fit(placeholder, width), fg(th().dim)))
    } else {
        Line::from(visible)
    };
    f.render_widget(Paragraph::new(line), inner);
    if focused {
        let col = (input.cursor - scroll).min(width.saturating_sub(1)) as u16;
        f.set_cursor_position((inner.x + col, inner.y));
    }
}

fn render_banner(f: &mut Frame, area: Rect, app: &App) {
    let Some((text, is_error)) = &app.banner else { return };
    let (mark, color) = if *is_error { ("✖", th().fail) } else { ("✔", th().ok) };
    let room = (area.width as usize).saturating_sub(3);
    f.render_widget(
        Line::from(vec![
            Span::styled(format!(" {mark} "), fg(color).bold()),
            Span::styled(fit(text, room), fg(color)),
        ]),
        area,
    );
}

// ------------------------------------------------------------------- the form

fn form(f: &mut Frame, area: Rect, app: &App) {
    let [head, urls, row1, row2, opts, banner, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    // The tagline types itself out when the app opens.
    let tagline = format!("{TAGLINE} · {}", app.git);
    let typed = if app.settings.motion { app.spinner.saturating_mul(2) } else { usize::MAX };
    let mut rest = vec![Span::styled(tagline.chars().take(typed).collect::<String>(), fg(th().muted))];
    if typed < tagline.chars().count() {
        rest.push(Span::styled("▍", fg(th().accent)));
    }
    header(f, head, app, th().accent, true, rest);

    render_urls(f, urls, app);

    let [dest, branch] = Layout::horizontal([Constraint::Min(20), Constraint::Length(24)]).areas(row1);
    render_input(
        f,
        dest,
        " destination folder ",
        &app.dest,
        app.field == Field::Dest,
        "current directory",
        false,
    );
    render_input(
        f,
        branch,
        " branch ",
        &app.branch,
        app.field == Field::Branch,
        "default",
        false,
    );

    let [user, token] = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(row2);
    render_input(
        f,
        user,
        " username (optional) ",
        &app.user,
        app.field == Field::User,
        "auto per host",
        false,
    );
    let token_title = if app.reveal {
        " token / PAT  (ctrl+t hides) "
    } else {
        " token / PAT  (ctrl+t shows) "
    };
    render_input(
        f,
        token,
        token_title,
        &app.token,
        app.field == Field::Token,
        "only needed for private repos",
        !app.reveal,
    );

    let chips = [
        (Field::Shallow, "shallow", on_off(app.shallow).to_string()),
        (Field::Submodules, "submodules", on_off(app.submodules).to_string()),
        (Field::Existing, "if folder exists", app.existing.label().to_string()),
        (Field::Parallel, "at once", app.parallel.to_string()),
    ];
    let mut spans = vec![Span::raw(" ")];
    for (field, label, value) in chips {
        if app.field == field {
            spans.push(Span::styled(format!(" ◂ {label} {value} ▸ "), fg(th().ink).bg(th().accent).bold()));
        } else {
            spans.push(Span::styled(format!(" {label} "), fg(th().dim)));
            spans.push(Span::styled(format!("{value} "), fg(th().text)));
        }
        spans.push(Span::raw(" "));
    }
    f.render_widget(Line::from(spans), opts);

    match update::available() {
        Some(v) if app.banner.is_none() => f.render_widget(
            Line::from(vec![
                // The command comes before the detail, so a narrow window
                // clips the detail rather than the command.
                Span::styled(" ↑ ", fg(th().warn).bold()),
                Span::styled(format!("v{v} is out — quit and run "), fg(th().warn)),
                Span::styled(" gitcloner --update ", fg(th().ink).bg(th().warn).bold()),
                Span::styled(format!("  you have v{}", env!("CARGO_PKG_VERSION")), fg(th().dim)),
            ]),
            banner,
        ),
        _ => render_banner(f, banner, app),
    }
    footer(f, help, &[("tab", "next field"), ("ctrl+s", "clone"), ("esc", "quit")]);
}

fn on_off(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

fn render_urls(f: &mut Frame, area: Rect, app: &App) {
    let count = app.urls.lines.iter().filter(|l| !l.text.trim().is_empty()).count();
    let title = format!(" repositories — one per line ({count}) ");
    let focused = app.field == Field::Urls;
    let block = field_block(&title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (row, col) = app.urls.cursor();
    let height = inner.height as usize;
    let width = inner.width as usize;
    let vscroll = row.saturating_sub(height.saturating_sub(1));
    let hscroll = col.saturating_sub(width.saturating_sub(1));

    if app.urls.lines.iter().all(|l| l.text.is_empty()) {
        let hint = vec![
            Line::from(Span::styled("paste repositories here — one per line, or a whole list at once", fg(th().muted))),
            Line::from(Span::styled("  https://github.com/owner/repo   ·   owner/repo   ·   git@github.com:owner/repo.git", fg(th().dim))),
            Line::from(vec![
                Span::styled("  https://github.com/myorg", fg(th().dim)),
                Span::styled("   lists an org's repos so you can pick", fg(th().accent)),
            ]),
        ];
        f.render_widget(Paragraph::new(hint), inner);
    } else {
        let lines: Vec<Line> = app.urls.lines.iter().map(|l| Line::from(l.text.clone())).collect();
        f.render_widget(Paragraph::new(lines).scroll((vscroll as u16, hscroll as u16)), inner);
    }
    if focused {
        f.set_cursor_position((
            inner.x + (col - hscroll) as u16,
            inner.y + (row - vscroll) as u16,
        ));
    }
}

// ------------------------------------------------------------------ org picker

fn picker(f: &mut Frame, area: Rect, app: &App) {
    let [head, list, detail, banner, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(6),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let p = &app.pick;
    let mut summary = if p.loading {
        format!("listing the repositories of {}", p.orgs)
    } else {
        let ticked = p.repos.iter().zip(&p.picked).filter(|(_, on)| **on);
        let (n, kb) = ticked.fold((0, 0), |(n, kb), (r, _)| (n + 1, kb + r.size_kb));
        format!("{} — {n} of {} ticked · about {} to fetch", p.orgs, p.repos.len(), size(kb))
    };
    if !p.rest.is_empty() {
        summary += &format!(" · plus {} other URL(s)", p.rest.len());
    }
    let dot = if p.loading { th().warn } else { th().accent };
    header(f, head, app, dot, true, vec![Span::styled(summary, fg(th().muted))]);

    let block = panel();
    let inner = block.inner(list);
    f.render_widget(block, list);
    if p.loading {
        skeleton(f, inner, tick(app));
    } else {
        repo_rows(f, inner, app);
    }
    repo_detail(f, detail, app);

    render_banner(f, banner, app);
    let keys: &[(&str, &str)] = if p.loading {
        &[("esc", "back"), ("q", "quit")]
    } else {
        &[
            ("↑↓", "move"),
            ("space", "tick"),
            ("a", "all/none"),
            ("enter", "clone ticked"),
            ("esc", "back"),
            ("q", "quit"),
        ]
    };
    footer(f, help, keys);
}

/// Placeholder rows shaped like the list that is on its way, with light
/// rippling down through them.
fn skeleton(f: &mut Frame, area: Rect, tick: usize) {
    let width = area.width as usize;
    let lines: Vec<Line> = (0..area.height as usize)
        .map(|row| {
            let bars = [12 + row * 7 % 15, 9, 6, 8, 10 + row * 11 % 30].map(|n| "━".repeat(n));
            let text: String = format!("   {}", bars.join("   ")).chars().take(width).collect();
            Line::from(sweep(&text, tick * 3 + row * 4, width + 12, fg(th().faint), fg(th().dim)))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn repo_rows(f: &mut Frame, area: Rect, app: &App) {
    let p = &app.pick;
    if area.height == 0 {
        return;
    }
    let height = area.height as usize;
    let width = area.width as usize;
    let scroll = p.row.saturating_sub(height.saturating_sub(1));
    let name_w = p
        .repos
        .iter()
        .map(|r| r.full_name.chars().count())
        .max()
        .unwrap_or(10)
        .clamp(8, 36);
    // marker + tick (3), name, then language 14, stars 8, size 10, pushed 10
    let meta = width >= 3 + name_w + 42 + 12;
    let shown = revealed(app);

    let lines: Vec<Line> = p
        .repos
        .iter()
        .zip(&p.picked)
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, (repo, on))| {
            if idx - scroll >= shown {
                return Line::default();
            }
            let here = idx == p.row;
            let name_style = if here {
                fg(th().text).bold()
            } else if *on {
                fg(th().text)
            } else {
                fg(th().dim)
            };
            let meta_style = if here { fg(th().muted) } else { fg(th().dim) };
            let mut spans = vec![
                Span::styled(if here { "▸" } else { " " }, fg(th().accent)),
                if *on { Span::styled("● ", fg(th().ok)) } else { Span::styled("○ ", fg(th().dim)) },
                Span::styled(pad(&repo.full_name, name_w), name_style),
            ];
            if meta {
                let dot = if repo.language.is_empty() { "    " } else { "  ● " };
                spans.push(Span::styled(dot, fg(lang_color(&repo.language))));
                spans.push(Span::styled(pad(&repo.language, 10), meta_style));
                spans.push(Span::styled(format!("{:>8}", format!("★ {}", count(repo.stars))), meta_style));
                spans.push(Span::styled(format!("{:>10}", size(repo.size_kb)), meta_style));
                spans.push(Span::styled(format!("{:>10}", ago(&repo.pushed)), meta_style));
            }
            let mut room = width.saturating_sub(spans.iter().map(Span::width).sum::<usize>() + 2);
            spans.push(Span::raw("  "));
            for (set, tag) in [(repo.private, "private"), (repo.fork, "fork"), (repo.archived, "archived")] {
                if set && room > tag.len() {
                    spans.push(Span::styled(format!("{tag} "), fg(th().warn)));
                    room -= tag.len() + 1;
                }
            }
            spans.push(Span::styled(fit(&repo.description, room), fg(th().dim)));
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

/// Everything GitHub told us about the highlighted repo.
fn repo_detail(f: &mut Frame, area: Rect, app: &App) {
    let p = &app.pick;
    let Some(repo) = p.repos.get(p.row).filter(|_| !p.loading) else {
        let block = panel();
        let inner = block.inner(area);
        f.render_widget(block, area);
        if p.loading {
            skeleton(f, inner, tick(app) + 7);
        }
        return;
    };

    let tags: Vec<&str> = [(repo.private, "private"), (repo.fork, "fork"), (repo.archived, "archived")]
        .into_iter()
        .filter_map(|(set, tag)| set.then_some(tag))
        .collect();
    let mut block = panel().title(Span::styled(format!(" {} ", repo.full_name), fg(th().text).bold()));
    if !tags.is_empty() {
        block = block.title_top(Line::from(Span::styled(format!(" {} ", tags.join(" · ")), fg(th().warn))).right_aligned());
    }

    let description = if repo.description.is_empty() {
        Line::from(Span::styled("no description", fg(th().dim).italic()))
    } else {
        Line::from(Span::styled(repo.description.clone(), fg(th().text)))
    };
    let facts = join(vec![
        some(
            !repo.language.is_empty(),
            vec![
                Span::styled("● ", fg(lang_color(&repo.language))),
                Span::styled(repo.language.clone(), fg(th().text)),
            ],
        ),
        stat(repo.stars, "stars"),
        stat(repo.forks, "forks"),
        stat(repo.issues, "open issues"),
        text(size(repo.size_kb)),
        some(!repo.license.is_empty(), labelled("license", repo.license.clone())),
    ]);
    let history = join(vec![
        some(!repo.branch.is_empty(), labelled("branch", repo.branch.clone())),
        some(
            !repo.pushed.is_empty(),
            labelled("last push", format!("{} ({})", ago(&repo.pushed), repo.pushed)),
        ),
        some(
            !repo.topics.is_empty(),
            vec![Span::styled(
                repo.topics.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "),
                fg(th().accent),
            )],
        ),
    ]);
    let url = Line::from(Span::styled(repo.url.clone(), fg(th().dim)));
    f.render_widget(Paragraph::new(vec![description, facts, history, url]).block(block), area);
}

// ---------------------------------------------------------------- progress view

fn look(status: Status, tick: usize) -> (&'static str, Color) {
    match status {
        Status::Cloned => ("✔", th().ok),
        Status::Failed => ("✖", th().fail),
        Status::Skipped => ("»", th().muted),
        Status::Cancelled => ("■", th().dim),
        Status::Invalid => ("!", th().fail),
        Status::Pending => ("·", th().dim),
        Status::Running => (SPINNER[tick % SPINNER.len()], th().accent),
    }
}

/// Wall-clock time from the first clone starting to the last one ending.
fn wall(app: &App) -> Option<f32> {
    let runs = app.jobs.iter().filter_map(|j| Some((j.started?, j.started? + j.elapsed?)));
    let first = runs.clone().map(|(start, _)| start).min()?;
    let last = runs.map(|(_, end)| end).max()?;
    Some((last - first).as_secs_f32())
}

fn progress(f: &mut Frame, area: Rect, app: &App) {
    let [head, overall, list, log, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let total = app.jobs.len();
    let done = total - app.jobs.iter().filter(|j| j.status.is_active()).count();
    let busy = app.busy();
    let failed = app.count(Status::Failed) + app.count(Status::Invalid) > 0;
    let mood = if busy {
        th().accent
    } else if failed {
        th().warn
    } else {
        th().ok
    };

    let mut parts = match wall(app).filter(|_| !busy) {
        Some(secs) => vec![Span::styled(format!("all done in {secs:.1}s"), fg(mood).bold())],
        None => vec![Span::styled(format!("{done}/{total} finished"), fg(th().muted))],
    };
    for (status, color, label) in [
        (Status::Cloned, th().ok, "cloned"),
        (Status::Running, th().accent, "running"),
        (Status::Failed, th().fail, "failed"),
        (Status::Skipped, th().muted, "skipped"),
        (Status::Cancelled, th().dim, "cancelled"),
        (Status::Invalid, th().fail, "unusable"),
    ] {
        let n = app.count(status);
        if n > 0 {
            parts.push(Span::styled(" · ", fg(th().dim)));
            parts.push(Span::styled(format!("{n} {label}"), fg(color)));
        }
    }
    header(f, head, app, mood, busy, parts);

    // The drawn percentage glides towards the real one (see `App::eased`).
    let bar_width = (overall.width as usize).saturating_sub(8);
    let mut spans = vec![Span::raw(" ")];
    spans.extend(bar(app.eased / 100.0, bar_width, mood, (busy && app.settings.motion).then_some(app.spinner)));
    spans.push(Span::styled(format!(" {:>3.0}%", app.eased), fg(th().muted)));
    f.render_widget(Line::from(spans), overall);

    render_list(f, list, app);
    render_log(f, log, app);

    let keys: &[(&str, &str)] = if busy {
        &[("↑↓", "select"), ("c", "cancel"), ("C", "cancel all"), ("+/-", "at once"), ("q", "quit")]
    } else {
        &[("↑↓", "select"), ("r", "retry"), ("R", "retry all"), ("b", "back to form"), ("q", "quit")]
    };
    footer(f, help, keys);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let block = panel();
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width < 20 {
        return;
    }

    let height = inner.height as usize;
    let scroll = app.sel.saturating_sub(height.saturating_sub(1));
    let width = inner.width as usize;
    let name_w = app
        .jobs
        .iter()
        .map(|j| j.label.chars().count())
        .max()
        .unwrap_or(10)
        .clamp(8, 28);
    // "▸✔ " + name + " " + bar + " 100% " + " 12.3s" + " " + status
    let bar_w = width.saturating_sub(name_w + 24).clamp(6, 24);
    let status_w = width.saturating_sub(name_w + bar_w + 18);
    let shown = revealed(app);

    let lines: Vec<Line> = app
        .jobs
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, job)| {
            if idx - scroll >= shown {
                return Line::default();
            }
            let (glyph, color) = look(job.status, tick(app) + idx);
            // A job that has just finished flashes its mark for a moment.
            let fresh = job
                .started
                .zip(job.elapsed)
                .is_some_and(|(s, e)| (s + e).elapsed() < Duration::from_millis(450));
            let glyph_style = if fresh && app.settings.motion { fg(th().ink).bg(color).bold() } else { fg(color) };
            let marker = if idx == app.sel { "▸" } else { " " };
            let time = job.secs().map(|s| format!("{s:>5.1}s")).unwrap_or_else(|| "      ".into());
            let tail = match job.status {
                Status::Running if !job.transfer.is_empty() => {
                    format!("{} · {}", job.phase, job.transfer)
                }
                Status::Cloned if !job.transfer.is_empty() => {
                    format!("done · {}", job.transfer.split(" | ").next().unwrap_or(""))
                }
                Status::Cloned | Status::Pending | Status::Running => job.phase.clone(),
                _ => job.detail.clone(),
            };
            let name_style = if idx == app.sel { fg(th().text).bold() } else { fg(th().muted) };
            let frac = f32::from(job.pct) / 100.0;
            let track = match job.status {
                Status::Running if job.pct == 0 => comet(tick(app) + idx * 5, bar_w),
                Status::Running => bar(frac, bar_w, th().accent, Some(tick(app) + idx * 3)),
                _ => bar(frac, bar_w, color, None),
            };
            let mut spans = vec![
                Span::styled(marker, fg(th().accent)),
                Span::styled(glyph, glyph_style),
                Span::raw(" "),
                Span::styled(pad(&job.label, name_w), name_style),
                Span::raw(" "),
            ];
            spans.extend(track);
            spans.push(Span::styled(format!(" {:>3}% ", job.pct), fg(th().dim)));
            spans.push(Span::styled(time, fg(th().dim)));
            spans.push(Span::styled(format!(" {}", fit(&tail, status_w)), fg(color)));
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_log(f: &mut Frame, area: Rect, app: &App) {
    let selected = app.jobs.get(app.sel);
    let title = selected
        .map(|j| format!(" {} ", fit(j.url(), area.width.saturating_sub(4) as usize)))
        .unwrap_or_else(|| " log ".into());
    let block = panel().title(Span::styled(title, fg(th().muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(job) = selected else { return };
    if inner.height == 0 {
        return;
    }

    let (glyph, color) = look(job.status, tick(app));
    let what = match job.status {
        Status::Running => format!("{} {}%", job.phase, job.pct),
        Status::Cloned => format!("cloned into {}", job.detail),
        Status::Pending => format!("queued — will clone into {}", job.detail),
        _ => job.detail.clone(),
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{glyph} "), fg(color)),
        Span::styled(what, fg(color)),
    ])];
    if let Some(j) = &job.job {
        let flag = |on: bool, name: &str| {
            if on { vec![Span::styled(name.to_string(), fg(th().text))] } else { Vec::new() }
        };
        lines.push(join(vec![
            vec![
                Span::styled("branch ", fg(th().dim)),
                Span::styled(j.branch.clone().unwrap_or_else(|| "default".into()), fg(th().text)),
            ],
            flag(j.shallow, "shallow"),
            flag(j.submodules, "submodules"),
            flag(j.token.is_some(), "with PAT"),
            flag(!job.transfer.is_empty(), &job.transfer),
            job.secs().map(|s| vec![Span::styled(format!("{s:.1}s"), fg(th().text))]).unwrap_or_default(),
        ]));
    }
    let room = (inner.height as usize).saturating_sub(lines.len());
    lines.extend(
        job.log
            .iter()
            .rev()
            .take(room)
            .rev()
            .map(|l| Line::from(Span::styled(l.clone(), fg(th().dim)))),
    );
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

// ---------------------------------------------------------------------- popups

/// A centred box that grows into place when it opens, over a cleared area.
fn popup(f: &mut Frame, area: Rect, app: &App, w: u16, h: u16) -> Rect {
    let grow = match app.popped {
        Some(t) if app.settings.motion => (t.elapsed().as_secs_f32() / 0.18).min(1.0),
        _ => 1.0,
    };
    let ease = 1.0 - (1.0 - grow).powi(3);
    let w = (f32::from(w.min(area.width)) * (0.75 + 0.25 * ease)) as u16;
    let h = (f32::from(h.min(area.height)) * (0.5 + 0.5 * ease)) as u16;
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    rect
}

fn titled<'a>(title: impl Into<Line<'a>>, color: Color) -> Block<'a> {
    panel().border_style(fg(color)).title(title)
}

fn heading(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {text}"), fg(th().muted).bold()))
}

fn indent(line: Line<'static>) -> Line<'static> {
    let mut spans = vec![Span::raw("   ")];
    spans.extend(line.spans);
    Line::from(spans)
}

fn text(s: String) -> Vec<Span<'static>> {
    vec![Span::styled(s, fg(th().text))]
}

fn labelled(label: &str, value: String) -> Vec<Span<'static>> {
    vec![Span::styled(format!("{label} "), fg(th().dim)), Span::styled(value, fg(th().text))]
}

fn stat(n: u64, what: &str) -> Vec<Span<'static>> {
    vec![Span::styled(count(n), fg(th().text)), Span::styled(format!(" {what}"), fg(th().dim))]
}

fn some(cond: bool, spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    if cond { spans } else { Vec::new() }
}

/// The yes / no questions.
fn render_modal(f: &mut Frame, area: Rect, app: &App, modal: Modal) {
    let (title, body, color) = match modal {
        Modal::ConfirmOverwrite(n) => (
            " delete and re-clone? ",
            format!(
                "{n} destination folder(s) already have files in them.\n\
                 \"{}\" means they get deleted first — this cannot be undone.\n\n\
                 y = delete and clone     n = go back and change it",
                OnExisting::Overwrite.label()
            ),
            th().fail,
        ),
        _ => (
            " quit? ",
            "Clones are still running. Quitting stops them and removes the\n\
             half-written folders.\n\n\
             y = stop and quit     n = keep going"
                .to_string(),
            th().warn,
        ),
    };
    let rect = popup(f, area, app, 66, 9);
    let block = titled(Span::styled(title, fg(color).bold()), color);
    f.render_widget(Paragraph::new(body).block(block).wrap(Wrap { trim: true }), rect);
}

/// Every key, by screen, and the URL forms. Empty key = section heading.
pub const HELP: &[(&str, &str)] = &[
    ("", "ANYWHERE"),
    ("F1  ctrl+g", "this help (? works too, outside the form)"),
    ("F2  ctrl+o", "settings: theme, animations, popup, update check"),
    ("F3  ctrl+a", "profile: your GitHub account; who made this"),
    ("esc", "close a panel"),
    ("ctrl+c", "quit (asks first while clones are running)"),
    ("", ""),
    ("", "THE FORM"),
    ("tab  shift+tab", "next / previous field"),
    ("ctrl+s  F5", "start cloning"),
    ("ctrl+t", "show / hide the token"),
    ("ctrl+u  ctrl+w", "clear the field / delete a word"),
    ("← → space", "change a toggle; 1-9 sets \"at once\""),
    ("esc", "quit"),
    ("", ""),
    ("", "ORG LIST"),
    ("↑ ↓  j k", "move"),
    ("space  a", "tick one / tick all or none"),
    ("enter", "clone what is ticked"),
    ("esc  b", "back to the form"),
    ("", ""),
    ("", "WHILE CLONING"),
    ("↑ ↓  j k", "select a repo"),
    ("c  C", "cancel the selected one / all of them"),
    ("r  R", "retry the selected one / everything that failed"),
    ("+  -", "more / fewer at once, live"),
    ("b", "back to the form, once finished"),
    ("", ""),
    ("", "WHEN A BATCH FINISHES"),
    ("enter", "close the summary"),
    ("o", "open the destination folder"),
    ("R", "retry everything that failed"),
    ("b", "back to the form"),
    ("", ""),
    ("", "PASTE ANY OF THESE"),
    ("owner/repo", "GitHub shorthand"),
    ("https://host/owner/repo", "GitHub, GitLab, Bitbucket, Azure, any host"),
    ("…/tree/<branch>", "a browser URL; the branch is used"),
    ("git@host:owner/repo", "SSH, with your own keys"),
    ("https://github.com/org", "pick from an org's or user's repos"),
    ("C:\\path\\to\\repo", "a repo on this machine"),
    ("", ""),
    ("", "FROM THE SHELL"),
    ("gitcloner --help", "every option"),
    ("gitcloner --update", "update to the latest release"),
];

fn help_panel(f: &mut Frame, area: Rect, app: &App) {
    let t = th();
    let rect = popup(f, area, app, 78, area.height.saturating_sub(4).max(12));
    let block = titled(Span::styled(" help ", fg(t.accent).bold()), t.accent)
        .title_bottom(Line::from(Span::styled(" ↑↓ scroll · esc close ", fg(t.dim))).right_aligned());
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let height = inner.height as usize;
    let scroll = app.menu_row.min(HELP.len().saturating_sub(height));
    let lines: Vec<Line> = HELP
        .iter()
        .skip(scroll)
        .take(height)
        .map(|(key, what)| {
            if key.is_empty() {
                heading(what)
            } else {
                Line::from(vec![
                    Span::styled(format!("   {}", pad(key, 24)), fg(t.accent)),
                    Span::styled(*what, fg(t.text)),
                ])
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn settings_panel(f: &mut Frame, area: Rect, app: &App) {
    let t = th();
    let s = &app.settings;
    let rect = popup(f, area, app, 78, 13);
    let block = titled(Span::styled(" settings ", fg(t.accent).bold()), t.accent).title_bottom(
        Line::from(Span::styled(" ↑↓ choose · ←→ change · esc close ", fg(t.dim))).right_aligned(),
    );
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let rows: [(&str, &str, &str); settings::ROWS] = [
        ("theme", t.name, t.about),
        ("animations", on_off(s.motion), "moving bars, shimmer, lists cascading in"),
        ("popup when done", on_off(s.popup), "a summary when a batch of clones ends"),
        ("check for updates", on_off(s.update_check), "one quick look at GitHub on start"),
    ];
    let mut lines = vec![Line::default()];
    for (i, (label, value, about)) in rows.into_iter().enumerate() {
        let here = i == app.menu_row;
        lines.push(Line::from(vec![
            Span::styled(if here { " ▸ " } else { "   " }, fg(t.accent)),
            Span::styled(pad(label, 19), if here { fg(t.text).bold() } else { fg(t.muted) }),
            if here {
                Span::styled(format!(" ◂ {value:<5} ▸ "), fg(t.ink).bg(t.accent).bold())
            } else {
                Span::styled(format!("   {value:<5}   "), fg(t.text))
            },
            Span::styled(format!("  {about}"), fg(t.dim)),
        ]));
    }
    // The theme, live: its colours and a working bar.
    let swatch = |c: Color| Span::styled("███ ", fg(c));
    let mut preview = vec![
        Span::raw("   "),
        swatch(t.accent),
        swatch(t.accent_hi),
        swatch(t.ok),
        swatch(t.warn),
        swatch(t.fail),
        Span::raw("  "),
    ];
    preview.extend(bar(0.62, 22, t.accent, s.motion.then_some(app.spinner)));
    lines.push(Line::default());
    lines.push(Line::from(preview));
    lines.push(Line::default());
    let saved = match &s.path {
        Some(path) => format!("saved to {}", path.display()),
        None => "not saved — no settings folder on this system".into(),
    };
    lines.push(Line::from(Span::styled(
        format!(" {}", fit(&saved, (inner.width as usize).saturating_sub(1))),
        fg(t.dim),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

fn profile_panel(f: &mut Frame, area: Rect, app: &App) {
    let t = th();
    let rect = popup(f, area, app, 78, 19);
    let block = titled(Span::styled(" profile ", fg(t.accent).bold()), t.accent)
        .title_bottom(Line::from(Span::styled(" r refresh · esc close ", fg(t.dim))).right_aligned());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines = vec![Line::default(), heading("YOU ON GITHUB")];
    match &app.account {
        _ if app.account_loading => {
            for (row, n) in [18, 34].into_iter().enumerate() {
                let text = format!("   {}", "━".repeat(n));
                lines.push(Line::from(sweep(&text, tick(app) * 3 + row * 4, 50, fg(t.faint), fg(t.dim))));
            }
        }
        Some(Ok(me)) => {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(me.login.clone(), fg(t.accent).bold()),
                Span::styled(format!("  {}", me.name), fg(t.text)),
            ]));
            lines.push(indent(join(vec![
                stat(me.public_repos, "public repos"),
                stat(me.private_repos, "private"),
                stat(me.followers, "followers"),
                stat(me.following, "following"),
            ])));
            lines.push(indent(join(vec![
                some(!me.company.is_empty(), text(me.company.clone())),
                some(!me.location.is_empty(), text(me.location.clone())),
                some(!me.since.is_empty(), labelled("on GitHub since", me.since.clone())),
            ])));
            if !me.bio.is_empty() {
                lines.push(Line::from(Span::styled(format!("   {}", me.bio), fg(t.dim).italic())));
            }
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(" enter ", fg(t.ink).bg(t.accent).bold()),
                Span::styled(" pick from your own repos", fg(t.muted)),
            ]));
        }
        Some(Err(why)) => {
            lines.push(Line::from(Span::styled(format!("   ✖ {why}"), fg(t.fail))));
            lines.push(Line::from(Span::styled("   check the token on the form, then press r", fg(t.dim))));
        }
        None => lines.push(Line::from(Span::styled(
            "   add a token on the form (or set $GITHUB_TOKEN) to see your account here",
            fg(t.dim),
        ))),
    }

    let author = env!("CARGO_PKG_AUTHORS");
    lines.push(Line::default());
    lines.push(heading("MADE BY"));
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled(author, fg(t.accent).bold()),
        Span::styled(format!("  github.com/{author}"), fg(t.muted)),
    ]));
    lines.push(indent(join(vec![
        text(format!("gitcloner v{}", env!("CARGO_PKG_VERSION"))),
        text("MIT license".into()),
        vec![Span::styled(env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://"), fg(t.dim))],
    ])));
    lines.push(match update::available() {
        Some(v) => Line::from(vec![
            Span::styled(format!("   ↑ v{v} is out — quit and run "), fg(t.warn)),
            Span::styled(" gitcloner --update ", fg(t.ink).bg(t.warn).bold()),
        ]),
        None if !app.settings.update_check => {
            Line::from(Span::styled("   update check is off (F2 settings)", fg(t.dim)))
        }
        None => Line::from(Span::styled("   ✔ no newer release found", fg(t.ok))),
    });
    lines.push(Line::from(Span::styled(format!("   {}", app.git), fg(t.dim))));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// What a finished batch came to, and what to do next.
fn done_panel(f: &mut Frame, area: Rect, app: &App) {
    let t = th();
    let broken: Vec<_> = app
        .jobs
        .iter()
        .filter(|j| matches!(j.status, Status::Failed | Status::Invalid))
        .collect();
    let (title, color) =
        if broken.is_empty() { (" ✔ all done ", t.ok) } else { (" finished, with problems ", t.warn) };
    let listed = broken.len().min(5);
    // borders, 4 lines of summary, the failures (+ a gap), the keys, a margin
    let more = usize::from(broken.len() > listed) + usize::from(!broken.is_empty());
    let rect = popup(f, area, app, 72, (2 + 4 + listed + more + 2) as u16);
    // The title catches the light once, as the box opens.
    let since = match app.popped {
        Some(at) if app.settings.motion => at.elapsed().as_millis() as usize / 40,
        _ => usize::MAX / 2,
    };
    let shine = sweep(title, since, usize::MAX, fg(color).bold(), fg(t.ink).bg(color).bold());
    let block = titled(Line::from(shine), color);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let width = inner.width as usize;

    let mut tally = vec![Span::raw("   ")];
    for (status, c, label) in [
        (Status::Cloned, t.ok, "cloned"),
        (Status::Failed, t.fail, "failed"),
        (Status::Skipped, t.muted, "skipped"),
        (Status::Cancelled, t.dim, "cancelled"),
        (Status::Invalid, t.fail, "unusable"),
    ] {
        let n = app.count(status);
        if n > 0 {
            if tally.len() > 1 {
                tally.push(Span::styled("  ·  ", fg(t.dim)));
            }
            tally.push(Span::styled(n.to_string(), fg(c).bold()));
            tally.push(Span::styled(format!(" {label}"), fg(c)));
        }
    }
    let dest = match app.dest.text.trim() {
        "" => "the current folder".to_string(),
        d => d.to_string(),
    };
    let took = wall(app).map(|s| format!("in {s:.1}s  →  ")).unwrap_or_default();

    let mut lines = vec![
        Line::default(),
        Line::from(tally),
        Line::from(Span::styled(format!("   {}", fit(&format!("{took}{dest}"), width.saturating_sub(3))), fg(t.muted))),
        Line::default(),
    ];
    for job in &broken[..listed] {
        let name = pad(&job.label, 18);
        let room = width.saturating_sub(name.chars().count() + 6);
        lines.push(Line::from(vec![
            Span::styled("   ✖ ", fg(t.fail)),
            Span::styled(name, fg(t.text)),
            Span::styled(format!(" {}", fit(&job.detail, room)), fg(t.dim)),
        ]));
    }
    if broken.len() > listed {
        lines.push(Line::from(Span::styled(format!("     … and {} more", broken.len() - listed), fg(t.dim))));
    }
    if !broken.is_empty() {
        lines.push(Line::default());
    }
    let retry = app.jobs.iter().any(|j| j.status.retryable());
    let mut keys = vec![("enter", "close"), ("o", "open folder")];
    if retry {
        keys.push(("R", "retry"));
    }
    keys.push(("b", "back to form"));
    lines.push(indent(hints(&keys)));
    f.render_widget(Paragraph::new(lines), inner);
}

// ---------------------------------------------------------------- numbers

fn lang_color(lang: &str) -> Color {
    let h = lang.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(usize::from(b)));
    LANG[h % LANG.len()]
}

/// 1204 -> "1.2k"
fn count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=9_999 => format!("{}.{}k", n / 1_000, n % 1_000 / 100),
        10_000..=999_999 => format!("{}k", n / 1_000),
        _ => format!("{}.{}M", n / 1_000_000, n % 1_000_000 / 100_000),
    }
}

fn size(kb: u64) -> String {
    match kb {
        0..=1023 => format!("{kb} KB"),
        1024..=1_048_575 => format!("{:.1} MB", kb as f64 / 1024.0),
        _ => format!("{:.1} GB", kb as f64 / 1_048_576.0),
    }
}

/// Days since 1970-01-01 for `YYYY-MM-DD` (Howard Hinnant's days_from_civil).
fn days(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// `YYYY-MM-DD` -> "3d ago", at most 8 characters.
fn ago(date: &str) -> String {
    let Some(then) = days(date) else { return String::new() };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i64);
    match now - then {
        d if d <= 0 => "today".into(),
        d if d < 31 => format!("{d}d ago"),
        d if d < 365 => format!("{}mo ago", d / 30),
        d => format!("{}y ago", d / 365),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_like_github() {
        assert_eq!(days("1970-01-01"), Some(0));
        assert_eq!(days("2000-03-01"), Some(11_017));
        assert_eq!(days("2026-10-15"), Some(20_741));
        assert_eq!(days("not-a-date"), None);
        assert_eq!(
            [999, 1_204, 9_999, 48_000, 2_350_000].map(count),
            ["999", "1.2k", "9.9k", "48k", "2.3M"]
        );
        assert_eq!([840, 12_700, 3_145_728].map(size), ["840 KB", "12.4 MB", "3.0 GB"]);
    }

    #[test]
    fn bars_fill_in_half_cells() {
        let text = |spans: Vec<Span>| spans.iter().map(|s| s.content.to_string()).collect::<String>();
        assert_eq!(text(bar(0.0, 4, th().ok, None)), "────");
        assert_eq!(text(bar(0.5, 4, th().ok, None)), "━━──");
        assert_eq!(text(bar(0.625, 4, th().ok, None)), "━━╸─");
        assert_eq!(text(bar(1.0, 4, th().ok, Some(1))), "━━━━");
        assert_eq!(text(comet(5, 4)).chars().count(), 4);
    }
}
