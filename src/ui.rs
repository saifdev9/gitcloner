//! Everything that draws. No state is changed here.
//!
//! Every animation is a pure function of `app.spinner` (one tick per 80 ms)
//! and the clock, so any frame can be drawn at any moment.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use crate::app::{App, Field, Input, Modal, Screen, Status};
use crate::clone::OnExisting;
use crate::update;

// 256-colour palette rather than truecolor, so it looks the same everywhere,
// macOS Terminal.app included. One accent; every other colour means a result.
const ACCENT: Color = Color::Indexed(75); // #5fafff
const ACCENT_HI: Color = Color::Indexed(153); // #afd7ff
const ACCENT_LO: Color = Color::Indexed(25); // #005faf
const OK: Color = Color::Indexed(114); // #87d787
const FAIL: Color = Color::Indexed(167); // #d75f5f
const WARN: Color = Color::Indexed(179); // #d7af5f
const TEXT: Color = Color::Reset;
const MUTED: Color = Color::Indexed(247);
const DIM: Color = Color::Indexed(243);
const FAINT: Color = Color::Indexed(238);
const INK: Color = Color::Indexed(234); // text on an accent background
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
    let area = f.area();
    if area.width < MIN_W || area.height < MIN_H {
        let msg = format!(
            "window is {}x{} — gitcloner needs at least {MIN_W}x{MIN_H}",
            area.width, area.height
        );
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }).fg(WARN).centered(), area);
        return;
    }
    match app.screen {
        Screen::Form => form(f, area, app),
        Screen::Pick => picker(f, area, app),
        Screen::Progress => progress(f, area, app),
    }
    if let Some(modal) = app.modal {
        render_modal(f, area, modal);
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
            spans.push(Span::styled("━", fg(ACCENT_HI)));
            spans.push(Span::styled("━".repeat(full - g - 1), fg(color)));
        }
        None => spans.push(Span::styled("━".repeat(full), fg(color))),
    }
    if half {
        spans.push(Span::styled("╸", fg(color)));
    }
    let used = full + usize::from(half);
    spans.push(Span::styled("─".repeat(width.saturating_sub(used)), fg(FAINT)));
    spans
}

/// No percentage yet (connecting, counting): a comet runs along the track.
fn comet(tick: usize, width: usize) -> Vec<Span<'static>> {
    let head = tick % (width + 4);
    (0..width)
        .map(|i| match head.checked_sub(i) {
            Some(0) => Span::styled("━", fg(ACCENT_HI)),
            Some(1) => Span::styled("━", fg(ACCENT)),
            Some(2 | 3) => Span::styled("━", fg(ACCENT_LO)),
            _ => Span::styled("─", fg(FAINT)),
        })
        .collect()
}

/// Brand badge with a glint passing through it, a breathing status dot,
/// whatever the screen has to say, and the byline on the right.
fn header(f: &mut Frame, area: Rect, app: &App, dot: Color, live: bool, rest: Vec<Span<'static>>) {
    let badge = fg(INK).bg(ACCENT).bold();
    let mut spans = sweep(" gitcloner ", app.spinner, 50, badge, fg(INK).bg(ACCENT_HI).bold());
    let pulse = if live { PULSE[app.spinner / 3 % PULSE.len()] } else { "●" };
    spans.push(Span::styled(format!(" {pulse} "), fg(dot)));
    spans.extend(rest);

    let by = Line::from(Span::styled(format!("  {BYLINE} "), fg(DIM))).right_aligned();
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
        spans.push(Span::styled(key.to_string(), fg(MUTED).bold()));
        spans.push(Span::styled(format!(" {what}   "), fg(DIM)));
    }
    Line::from(spans)
}

/// Groups of spans with a quiet separator between the non-empty ones.
fn join(groups: Vec<Vec<Span<'static>>>) -> Line<'static> {
    let mut spans = Vec::new();
    for group in groups.into_iter().filter(|g| !g.is_empty()) {
        if !spans.is_empty() {
            spans.push(Span::styled("  ·  ", fg(DIM)));
        }
        spans.extend(group);
    }
    Line::from(spans)
}

/// Rows of a list that has just appeared cascade in, one every 20 ms.
fn revealed(app: &App) -> usize {
    app.entered.map_or(usize::MAX, |t| t.elapsed().as_millis() as usize / 20 + 1)
}

fn panel<'a>() -> Block<'a> {
    Block::bordered().border_type(BorderType::Rounded).border_style(fg(FAINT))
}

fn field_block(title: &str, focused: bool) -> Block<'_> {
    if focused {
        panel().border_style(fg(ACCENT)).title(Span::styled(title, fg(ACCENT).bold()))
    } else {
        panel().title(Span::styled(title, fg(MUTED)))
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
        Line::from(Span::styled(fit(placeholder, width), fg(DIM)))
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
    let (mark, color) = if *is_error { ("✖", FAIL) } else { ("✔", OK) };
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
    let typed = app.spinner.saturating_mul(2);
    let mut rest = vec![Span::styled(tagline.chars().take(typed).collect::<String>(), fg(MUTED))];
    if typed < tagline.chars().count() {
        rest.push(Span::styled("▍", fg(ACCENT)));
    }
    header(f, head, app, ACCENT, true, rest);

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
            spans.push(Span::styled(format!(" ◂ {label} {value} ▸ "), fg(INK).bg(ACCENT).bold()));
        } else {
            spans.push(Span::styled(format!(" {label} "), fg(DIM)));
            spans.push(Span::styled(format!("{value} "), fg(TEXT)));
        }
        spans.push(Span::raw(" "));
    }
    f.render_widget(Line::from(spans), opts);

    match update::available() {
        Some(v) if app.banner.is_none() => f.render_widget(
            Line::from(vec![
                // The command comes before the detail, so a narrow window
                // clips the detail rather than the command.
                Span::styled(" ↑ ", fg(WARN).bold()),
                Span::styled(format!("v{v} is out — quit and run "), fg(WARN)),
                Span::styled(" gitcloner --update ", fg(INK).bg(WARN).bold()),
                Span::styled(format!("  you have v{}", env!("CARGO_PKG_VERSION")), fg(DIM)),
            ]),
            banner,
        ),
        _ => render_banner(f, banner, app),
    }
    f.render_widget(
        hints(&[
            ("tab", "next field"),
            ("ctrl+s", "clone"),
            ("ctrl+t", "show token"),
            ("ctrl+u", "clear field"),
            ("esc", "quit"),
        ]),
        help,
    );
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
            Line::from(Span::styled("paste repositories here — one per line, or a whole list at once", fg(MUTED))),
            Line::from(Span::styled("  https://github.com/owner/repo   ·   owner/repo   ·   git@github.com:owner/repo.git", fg(DIM))),
            Line::from(vec![
                Span::styled("  https://github.com/myorg", fg(DIM)),
                Span::styled("   lists an org's repos so you can pick", fg(ACCENT)),
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
    let dot = if p.loading { WARN } else { ACCENT };
    header(f, head, app, dot, true, vec![Span::styled(summary, fg(MUTED))]);

    let block = panel();
    let inner = block.inner(list);
    f.render_widget(block, list);
    if p.loading {
        skeleton(f, inner, app.spinner);
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
    f.render_widget(hints(keys), help);
}

/// Placeholder rows shaped like the list that is on its way, with light
/// rippling down through them.
fn skeleton(f: &mut Frame, area: Rect, tick: usize) {
    let width = area.width as usize;
    let lines: Vec<Line> = (0..area.height as usize)
        .map(|row| {
            let bars = [12 + row * 7 % 15, 9, 6, 8, 10 + row * 11 % 30].map(|n| "━".repeat(n));
            let text: String = format!("   {}", bars.join("   ")).chars().take(width).collect();
            Line::from(sweep(&text, tick * 3 + row * 4, width + 12, fg(FAINT), fg(DIM)))
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
                fg(TEXT).bold()
            } else if *on {
                fg(TEXT)
            } else {
                fg(DIM)
            };
            let meta_style = if here { fg(MUTED) } else { fg(DIM) };
            let mut spans = vec![
                Span::styled(if here { "▸" } else { " " }, fg(ACCENT)),
                if *on { Span::styled("● ", fg(OK)) } else { Span::styled("○ ", fg(DIM)) },
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
                    spans.push(Span::styled(format!("{tag} "), fg(WARN)));
                    room -= tag.len() + 1;
                }
            }
            spans.push(Span::styled(fit(&repo.description, room), fg(DIM)));
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
            skeleton(f, inner, app.spinner + 7);
        }
        return;
    };

    let tags: Vec<&str> = [(repo.private, "private"), (repo.fork, "fork"), (repo.archived, "archived")]
        .into_iter()
        .filter_map(|(set, tag)| set.then_some(tag))
        .collect();
    let mut block = panel().title(Span::styled(format!(" {} ", repo.full_name), fg(TEXT).bold()));
    if !tags.is_empty() {
        block = block.title_top(Line::from(Span::styled(format!(" {} ", tags.join(" · ")), fg(WARN))).right_aligned());
    }

    let text = |s: String| vec![Span::styled(s, fg(TEXT))];
    let labelled = |label: &str, value: String| {
        vec![Span::styled(format!("{label} "), fg(DIM)), Span::styled(value, fg(TEXT))]
    };
    let stat = |n: u64, what: &str| {
        vec![Span::styled(count(n), fg(TEXT)), Span::styled(format!(" {what}"), fg(DIM))]
    };
    let some = |cond: bool, spans: Vec<Span<'static>>| if cond { spans } else { Vec::new() };

    let description = if repo.description.is_empty() {
        Line::from(Span::styled("no description", fg(DIM).italic()))
    } else {
        Line::from(Span::styled(repo.description.clone(), fg(TEXT)))
    };
    let facts = join(vec![
        some(
            !repo.language.is_empty(),
            vec![
                Span::styled("● ", fg(lang_color(&repo.language))),
                Span::styled(repo.language.clone(), fg(TEXT)),
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
                fg(ACCENT),
            )],
        ),
    ]);
    let url = Line::from(Span::styled(repo.url.clone(), fg(DIM)));
    f.render_widget(Paragraph::new(vec![description, facts, history, url]).block(block), area);
}

// ---------------------------------------------------------------- progress view

fn look(status: Status, tick: usize) -> (&'static str, Color) {
    match status {
        Status::Cloned => ("✔", OK),
        Status::Failed => ("✖", FAIL),
        Status::Skipped => ("»", MUTED),
        Status::Cancelled => ("■", DIM),
        Status::Invalid => ("!", FAIL),
        Status::Pending => ("·", DIM),
        Status::Running => (SPINNER[tick % SPINNER.len()], ACCENT),
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
        ACCENT
    } else if failed {
        WARN
    } else {
        OK
    };

    let mut parts = match wall(app).filter(|_| !busy) {
        Some(secs) => vec![Span::styled(format!("all done in {secs:.1}s"), fg(mood).bold())],
        None => vec![Span::styled(format!("{done}/{total} finished"), fg(MUTED))],
    };
    for (status, color, label) in [
        (Status::Cloned, OK, "cloned"),
        (Status::Running, ACCENT, "running"),
        (Status::Failed, FAIL, "failed"),
        (Status::Skipped, MUTED, "skipped"),
        (Status::Cancelled, DIM, "cancelled"),
        (Status::Invalid, FAIL, "unusable"),
    ] {
        let n = app.count(status);
        if n > 0 {
            parts.push(Span::styled(" · ", fg(DIM)));
            parts.push(Span::styled(format!("{n} {label}"), fg(color)));
        }
    }
    header(f, head, app, mood, busy, parts);

    // The drawn percentage glides towards the real one (see `App::eased`).
    let bar_width = (overall.width as usize).saturating_sub(8);
    let mut spans = vec![Span::raw(" ")];
    spans.extend(bar(app.eased / 100.0, bar_width, mood, busy.then_some(app.spinner)));
    spans.push(Span::styled(format!(" {:>3.0}%", app.eased), fg(MUTED)));
    f.render_widget(Line::from(spans), overall);

    render_list(f, list, app);
    render_log(f, log, app);

    let keys: &[(&str, &str)] = if busy {
        &[("↑↓", "select"), ("c", "cancel"), ("C", "cancel all"), ("+/-", "at once"), ("q", "quit")]
    } else {
        &[("↑↓", "select"), ("r", "retry"), ("R", "retry all"), ("b", "back to form"), ("q", "quit")]
    };
    f.render_widget(hints(keys), help);
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
            let (glyph, color) = look(job.status, app.spinner + idx);
            // A job that has just finished flashes its mark for a moment.
            let fresh = job
                .started
                .zip(job.elapsed)
                .is_some_and(|(s, e)| (s + e).elapsed() < Duration::from_millis(450));
            let glyph_style = if fresh { fg(INK).bg(color).bold() } else { fg(color) };
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
            let name_style = if idx == app.sel { fg(TEXT).bold() } else { fg(MUTED) };
            let frac = f32::from(job.pct) / 100.0;
            let track = match job.status {
                Status::Running if job.pct == 0 => comet(app.spinner + idx * 5, bar_w),
                Status::Running => bar(frac, bar_w, ACCENT, Some(app.spinner + idx * 3)),
                _ => bar(frac, bar_w, color, None),
            };
            let mut spans = vec![
                Span::styled(marker, fg(ACCENT)),
                Span::styled(glyph, glyph_style),
                Span::raw(" "),
                Span::styled(pad(&job.label, name_w), name_style),
                Span::raw(" "),
            ];
            spans.extend(track);
            spans.push(Span::styled(format!(" {:>3}% ", job.pct), fg(DIM)));
            spans.push(Span::styled(time, fg(DIM)));
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
    let block = panel().title(Span::styled(title, fg(MUTED)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(job) = selected else { return };
    if inner.height == 0 {
        return;
    }

    let (glyph, color) = look(job.status, app.spinner);
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
            if on { vec![Span::styled(name.to_string(), fg(TEXT))] } else { Vec::new() }
        };
        lines.push(join(vec![
            vec![
                Span::styled("branch ", fg(DIM)),
                Span::styled(j.branch.clone().unwrap_or_else(|| "default".into()), fg(TEXT)),
            ],
            flag(j.shallow, "shallow"),
            flag(j.submodules, "submodules"),
            flag(j.token.is_some(), "with PAT"),
            flag(!job.transfer.is_empty(), &job.transfer),
            job.secs().map(|s| vec![Span::styled(format!("{s:.1}s"), fg(TEXT))]).unwrap_or_default(),
        ]));
    }
    let room = (inner.height as usize).saturating_sub(lines.len());
    lines.extend(
        job.log
            .iter()
            .rev()
            .take(room)
            .rev()
            .map(|l| Line::from(Span::styled(l.clone(), fg(DIM)))),
    );
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

// ----------------------------------------------------------------------- modal

fn render_modal(f: &mut Frame, area: Rect, modal: Modal) {
    let (title, body, color) = match modal {
        Modal::ConfirmOverwrite(n) => (
            " delete and re-clone? ",
            format!(
                "{n} destination folder(s) already have files in them.\n\
                 \"{}\" means they get deleted first — this cannot be undone.\n\n\
                 y = delete and clone     n = go back and change it",
                OnExisting::Overwrite.label()
            ),
            FAIL,
        ),
        Modal::ConfirmQuit => (
            " quit? ",
            "Clones are still running. Quitting stops them and removes the\n\
             half-written folders.\n\n\
             y = stop and quit     n = keep going"
                .to_string(),
            WARN,
        ),
    };
    let w = area.width.clamp(30, 66);
    let h = 9.min(area.height);
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let block = panel()
        .border_style(fg(color))
        .title(Span::styled(title, fg(color).bold()));
    f.render_widget(Paragraph::new(body).block(block).wrap(Wrap { trim: true }), rect);
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
        assert_eq!(text(bar(0.0, 4, OK, None)), "────");
        assert_eq!(text(bar(0.5, 4, OK, None)), "━━──");
        assert_eq!(text(bar(0.625, 4, OK, None)), "━━╸─");
        assert_eq!(text(bar(1.0, 4, OK, Some(1))), "━━━━");
        assert_eq!(text(comet(5, 4)).chars().count(), 4);
    }
}
