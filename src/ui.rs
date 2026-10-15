//! Everything that draws. No state is changed here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use crate::app::{App, Field, Input, Modal, Screen, Status};
use crate::clone::OnExisting;

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MIN_W: u16 = 60;
const MIN_H: u16 = 15;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    if area.width < MIN_W || area.height < MIN_H {
        let msg = format!(
            "window is {}x{} — gitcloner needs at least {MIN_W}x{MIN_H}",
            area.width, area.height
        );
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }).yellow().centered(), area);
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

fn bar(pct: u16, width: usize, color: Color) -> Span<'static> {
    let filled = (usize::from(pct.min(100)) * width).div_ceil(100);
    let text = format!("{}{}", "█".repeat(filled), "░".repeat(width.saturating_sub(filled)));
    Span::styled(text, Style::default().fg(color))
}

fn field_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    let block = Block::bordered().border_type(BorderType::Rounded).title(Span::from(title));
    if focused {
        block.border_style(Style::default().fg(ACCENT)).title_style(Style::default().fg(ACCENT).bold())
    } else {
        block.border_style(Style::default().fg(DIM))
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
        Line::from(Span::styled(fit(placeholder, width), Style::default().fg(DIM)))
    } else {
        Line::from(visible)
    };
    f.render_widget(Paragraph::new(line), inner);
    if focused {
        let col = (input.cursor - scroll).min(width.saturating_sub(1)) as u16;
        f.set_cursor_position((inner.x + col, inner.y));
    }
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

    f.render_widget(
        Line::from(vec![
            Span::styled(" gitcloner ", Style::default().fg(Color::Black).bg(ACCENT).bold()),
            Span::styled(format!(" clone many repos — public or private · {}", app.git), Style::default().fg(DIM)),
        ]),
        head,
    );

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
        (Field::Shallow, format!(" shallow: {} ", on_off(app.shallow))),
        (Field::Submodules, format!(" submodules: {} ", on_off(app.submodules))),
        (Field::Existing, format!(" if folder exists: {} ", app.existing.label())),
        (Field::Parallel, format!(" at once: {} ", app.parallel)),
    ];
    let spans: Vec<Span> = chips
        .iter()
        .map(|(field, text)| {
            if app.field == *field {
                Span::styled(text.clone(), Style::default().fg(Color::Black).bg(ACCENT).bold())
            } else {
                Span::styled(text.clone(), Style::default().fg(Color::Gray))
            }
        })
        .collect();
    f.render_widget(Line::from(spans), opts);

    render_banner(f, banner, app);
    f.render_widget(
        Line::from(Span::styled(
            " tab/shift+tab move · ctrl+s start · ctrl+t reveal token · ctrl+u clear field · esc quit",
            Style::default().fg(DIM),
        )),
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
            Line::from(Span::styled("https://github.com/owner/repo", Style::default().fg(DIM))),
            Line::from(Span::styled("owner/repo   ·   git@github.com:owner/repo.git", Style::default().fg(DIM))),
            Line::from(Span::styled("https://github.com/myorg — pick from an org's repos", Style::default().fg(DIM))),
            Line::from(Span::styled("paste a whole list — one per line", Style::default().fg(DIM))),
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

fn render_banner(f: &mut Frame, area: Rect, app: &App) {
    let Some((text, is_error)) = &app.banner else { return };
    let style = if *is_error {
        Style::default().fg(Color::Red).bold()
    } else {
        Style::default().fg(Color::Green)
    };
    let width = area.width as usize;
    f.render_widget(Line::from(Span::styled(fit(&format!(" {text}"), width), style)), area);
}

// ------------------------------------------------------------------ org picker

fn picker(f: &mut Frame, area: Rect, app: &App) {
    let [head, list, banner, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let p = &app.pick;
    let mut summary = if p.loading {
        format!(" listing the repositories of {} {}", p.orgs, SPINNER[app.spinner % SPINNER.len()])
    } else {
        let ticked = p.picked.iter().filter(|on| **on).count();
        format!(" {} — {ticked} of {} ticked", p.orgs, p.repos.len())
    };
    if !p.rest.is_empty() {
        summary += &format!(" · plus {} other URL(s)", p.rest.len());
    }
    let room = (head.width as usize).saturating_sub(11);
    f.render_widget(
        Line::from(vec![
            Span::styled(" gitcloner ", Style::default().fg(Color::Black).bg(ACCENT).bold()),
            Span::styled(fit(&summary, room), Style::default().fg(Color::Gray)),
        ]),
        head,
    );

    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::default().fg(DIM));
    let inner = block.inner(list);
    f.render_widget(block, list);
    if inner.height > 0 {
        let height = inner.height as usize;
        let scroll = p.row.saturating_sub(height.saturating_sub(1));
        let name_w = p
            .repos
            .iter()
            .map(|r| r.full_name.chars().count())
            .max()
            .unwrap_or(10)
            .clamp(8, 36);
        // "▸[x] " + name + " " + tags + " " + description
        let desc_w = (inner.width as usize).saturating_sub(name_w + 25);
        let lines: Vec<Line> = p
            .repos
            .iter()
            .zip(&p.picked)
            .enumerate()
            .skip(scroll)
            .take(height)
            .map(|(idx, (repo, on))| {
                let tags: Vec<&str> = [(repo.private, "private"), (repo.fork, "fork"), (repo.archived, "archived")]
                    .iter()
                    .filter(|(set, _)| *set)
                    .map(|(_, tag)| *tag)
                    .collect();
                let marker = if idx == p.row { "▸" } else { " " };
                let (check, color) = if *on { ("[x]", Color::Green) } else { ("[ ]", DIM) };
                let name_style = if idx == p.row {
                    Style::default().fg(Color::White).bold()
                } else if *on {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default().fg(DIM)
                };
                Line::from(vec![
                    Span::styled(format!("{marker}{check} "), Style::default().fg(color)),
                    Span::styled(pad(&repo.full_name, name_w), name_style),
                    Span::styled(format!(" {}", pad(&tags.join(" · "), 18)), Style::default().fg(Color::Yellow)),
                    Span::styled(format!(" {}", fit(&repo.description, desc_w)), Style::default().fg(DIM)),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    }

    render_banner(f, banner, app);
    let hints = if p.loading {
        " esc back to form · q quit"
    } else {
        " ↑↓ move · space tick · a all/none · enter clone ticked · esc back · q quit"
    };
    f.render_widget(Line::from(Span::styled(hints, Style::default().fg(DIM))), help);
}

// ---------------------------------------------------------------- progress view

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
    let mut parts = vec![
        Span::styled(" gitcloner ", Style::default().fg(Color::Black).bg(ACCENT).bold()),
        Span::styled(format!(" {done}/{total} finished"), Style::default().fg(Color::Gray)),
    ];
    for (status, color, label) in [
        (Status::Cloned, Color::Green, "cloned"),
        (Status::Running, Color::Yellow, "running"),
        (Status::Failed, Color::Red, "failed"),
        (Status::Skipped, Color::Blue, "skipped"),
        (Status::Cancelled, DIM, "cancelled"),
        (Status::Invalid, Color::Magenta, "unusable"),
    ] {
        let n = app.count(status);
        if n > 0 {
            parts.push(Span::styled(format!(" · {n} {label}"), Style::default().fg(color)));
        }
    }
    f.render_widget(Line::from(parts), head);

    let sum: u32 = app
        .jobs
        .iter()
        .map(|j| if j.status.is_active() { u32::from(j.pct) } else { 100 })
        .sum();
    let pct = if total == 0 { 0 } else { (sum / total as u32) as u16 };
    let bar_width = (overall.width as usize).saturating_sub(8);
    f.render_widget(
        Line::from(vec![
            Span::raw(" "),
            bar(pct, bar_width, ACCENT),
            Span::styled(format!(" {pct:>3}%"), Style::default().fg(Color::Gray)),
        ]),
        overall,
    );

    render_list(f, list, app);
    render_log(f, log, app);

    let hints = if app.busy() {
        " ↑↓ select · c cancel · C cancel all · +/- at once · q quit"
    } else {
        " ↑↓ select · r retry · R retry all · b back to form · q quit"
    };
    f.render_widget(Line::from(Span::styled(hints, Style::default().fg(DIM))), help);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::default().fg(DIM));
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

    let lines: Vec<Line> = app
        .jobs
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, job)| {
            let (glyph, color) = match job.status {
                Status::Cloned => ("✔".to_string(), Color::Green),
                Status::Failed => ("✖".to_string(), Color::Red),
                Status::Skipped => ("»".to_string(), Color::Blue),
                Status::Cancelled => ("■".to_string(), DIM),
                Status::Invalid => ("!".to_string(), Color::Magenta),
                Status::Pending => ("·".to_string(), DIM),
                Status::Running => (SPINNER[app.spinner % SPINNER.len()].to_string(), Color::Yellow),
            };
            let marker = if idx == app.sel { "▸" } else { " " };
            let time = job.secs().map(|s| format!("{s:>5.1}s")).unwrap_or_else(|| "      ".into());
            let tail = match job.status {
                Status::Cloned | Status::Pending | Status::Running => job.phase.clone(),
                _ => job.detail.clone(),
            };
            let name_style = if idx == app.sel {
                Style::default().fg(Color::White).bold()
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(vec![
                Span::styled(format!("{marker}{glyph} "), Style::default().fg(color)),
                Span::styled(pad(&job.label, name_w), name_style),
                Span::raw(" "),
                bar(job.pct, bar_w, color),
                Span::styled(format!(" {:>3}% ", job.pct), Style::default().fg(DIM)),
                Span::styled(time, Style::default().fg(DIM)),
                Span::styled(format!(" {}", fit(&tail, status_w)), Style::default().fg(color)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_log(f: &mut Frame, area: Rect, app: &App) {
    let selected = app.jobs.get(app.sel);
    let title = selected
        .map(|j| format!(" {} ", fit(j.url(), area.width.saturating_sub(4) as usize)))
        .unwrap_or_else(|| " log ".into());
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(title, Style::default().fg(Color::Gray)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(job) = selected else { return };
    if inner.height == 0 {
        return;
    }

    let detail_color = match job.status {
        Status::Failed | Status::Invalid => Color::Red,
        Status::Cloned => Color::Green,
        Status::Skipped => Color::Blue,
        _ => Color::Gray,
    };
    let mut lines = vec![Line::from(Span::styled(job.detail.clone(), Style::default().fg(detail_color)))];
    let room = (inner.height as usize).saturating_sub(1);
    lines.extend(
        job.log
            .iter()
            .rev()
            .take(room)
            .rev()
            .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(DIM)))),
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
            Color::Red,
        ),
        Modal::ConfirmQuit => (
            " quit? ",
            "Clones are still running. Quitting stops them and removes the\n\
             half-written folders.\n\n\
             y = stop and quit     n = keep going"
                .to_string(),
            Color::Yellow,
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
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(Span::styled(title, Style::default().fg(color).bold()));
    f.render_widget(Paragraph::new(body).block(block).wrap(Wrap { trim: true }), rect);
}
