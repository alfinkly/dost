use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use super::{App, ScreenKind, Tab};
use crate::runner::{build_plan, display_command};
use crate::stream::RunMode;
use crate::summary::summary_for;

const MIN_W: u16 = 80;
const MIN_H: u16 = 24;

fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub fn draw(app: &App, f: &mut Frame) {
    let area = f.area();
    if area.width < MIN_W || area.height < MIN_H {
        f.render_widget(
            Paragraph::new(format!(
                "Terminal too small: need at least {MIN_W}x{MIN_H}, have {}x{}.",
                area.width, area.height
            ))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("dost", bold()),
            Span::raw("  "),
            Span::styled(title(app), dim()),
        ])),
        head,
    );
    let footer = match app.screen {
        ScreenKind::Menu => "↑↓ choose · Enter open · q quit",
        ScreenKind::Preflight => "Enter run check · Esc back",
        ScreenKind::Password => "Enter confirm · Esc cancel",
        ScreenKind::Running => "Tab switch view · q cancel",
        ScreenKind::ApplyConfirm => "Enter apply · Esc cancel",
        ScreenKind::CheckFailed => {
            if app.command().needs_become {
                "r retry · p retry with a new password · a apply anyway · Enter back"
            } else {
                "r retry · a apply anyway · Enter back"
            }
        }
        ScreenKind::Result => {
            if !app.run_failed() {
                "Enter back to menu"
            } else if app.command().needs_become {
                "r retry · p retry with a new password · Enter back"
            } else {
                "r retry · Enter back"
            }
        }
    };
    f.render_widget(Paragraph::new(footer).style(dim()), foot);
    match app.screen {
        ScreenKind::Menu => menu(app, f, body),
        ScreenKind::Preflight => preflight(app, f, body),
        ScreenKind::Password => password(app, f, body),
        ScreenKind::Running => running(app, f, body),
        ScreenKind::ApplyConfirm => apply_confirm(app, f, body),
        ScreenKind::CheckFailed => check_failed(app, f, body),
        ScreenKind::Result => result(app, f, body),
    }
}

fn title(app: &App) -> String {
    match app.screen {
        ScreenKind::Menu => "menu".into(),
        _ => app.command().title.clone(),
    }
}

fn menu(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    for (sel, name, about) in app.menu_rows() {
        let marker = if sel { "▸ " } else { "  " };
        let style = if sel { bold() } else { Style::default() };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{name}"), style),
            Span::styled(format!("  {about}"), dim()),
        ]));
    }
    if let Some(m) = &app.message {
        lines.push(Line::raw(""));
        lines.push(Line::raw(m.clone()));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn command_line(app: &App, mode: RunMode) -> String {
    let plan = build_plan(
        &app.project.root,
        app.command(),
        mode,
        std::path::Path::new("<dost callback>"),
    );
    display_command(&plan)
}

fn preflight(app: &App, f: &mut Frame, area: Rect) {
    let c = app.command();
    let mut lines = vec![
        Line::from(Span::styled(c.title.clone(), bold())),
        Line::raw(c.about.clone()),
        Line::raw(""),
        Line::raw(format!("danger: {:?}", c.danger).to_lowercase()),
    ];
    if c.check {
        lines.push(Line::raw(
            "A check (--check --diff) runs first; nothing is changed.",
        ));
        lines.push(Line::raw(format!(
            "$ {}",
            command_line(app, RunMode::Check)
        )));
    } else {
        lines.push(Line::raw(
            "This command has check disabled: it will be applied WITHOUT a check.",
        ));
        lines.push(Line::raw(format!(
            "$ {}",
            command_line(app, RunMode::Apply)
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn password(app: &App, f: &mut Frame, area: Rect) {
    let dots = "•".repeat(app.pw_len);
    let lines = vec![
        Line::raw(format!("sudo password for {}:", app.command().title)),
        Line::raw(""),
        Line::from(Span::styled(format!("> {dots}"), bold())),
        Line::raw(""),
        Line::styled(
            "The password is kept in memory only and is never written to disk.",
            dim(),
        ),
    ];
    let mut lines = lines;
    if let Some(m) = &app.message {
        lines.push(Line::raw(""));
        lines.push(Line::raw(m.clone()));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn running(app: &App, f: &mut Frame, area: Rect) {
    let [tabs, content] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    let label = |t: Tab, name: &str| {
        if app.tab == t {
            Span::styled(format!("[{name}] "), bold())
        } else {
            Span::styled(format!(" {name}  "), dim())
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            label(Tab::Scene, "scene"),
            label(Tab::Log, "log"),
            label(Tab::Raw, "raw"),
            Span::styled(
                format!(
                    "  {}",
                    if app.running_mode == RunMode::Check {
                        "check"
                    } else {
                        "apply"
                    }
                ),
                dim(),
            ),
        ])),
        tabs,
    );
    match app.tab {
        Tab::Scene => {
            let id = app
                .command()
                .scene
                .clone()
                .unwrap_or_else(|| "chain".into());
            if let Some(scene) = app.scenes.get(&id).or_else(|| app.scenes.get("chain")) {
                scene.render(&app.state, content, f.buffer_mut());
            }
        }
        Tab::Log => {
            let lines: Vec<Line> = app
                .state
                .results
                .iter()
                .rev()
                .take(content.height as usize)
                .rev()
                .map(|r| {
                    let mark = if r.failed || r.unreachable {
                        "✗"
                    } else if r.changed {
                        "~"
                    } else {
                        "✓"
                    };
                    Line::raw(format!("{mark} {}: {}", r.host, r.task))
                })
                .collect();
            f.render_widget(Paragraph::new(lines), content);
        }
        Tab::Raw => {
            let text = if app.last_tail.is_empty() {
                "(no raw output yet)".to_string()
            } else {
                app.last_tail.clone()
            };
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), content);
        }
    }
}

fn apply_confirm(app: &App, f: &mut Frame, area: Rect) {
    let c = app.command();
    let required = app.forced || c.danger != crate::catalog::Danger::Low;
    let mut lines = Vec::new();
    if app.forced {
        lines.push(Line::styled(
            "The check FAILED. Applying anyway is your decision.",
            bold(),
        ));
    } else if app.skipped_check {
        lines.push(Line::styled(
            "No check will run: this command has check disabled.",
            bold(),
        ));
    } else {
        lines.push(Line::styled("Check passed.", bold()));
    }
    lines.push(Line::raw(format!(
        "$ {}",
        command_line(app, RunMode::Apply)
    )));
    lines.push(Line::raw(""));
    if required {
        lines.push(Line::raw(format!(
            "Type `{}` and press Enter to apply:",
            c.id
        )));
        lines.push(Line::from(Span::styled(format!("> {}", app.typed), bold())));
    } else {
        lines.push(Line::raw("Press Enter to apply."));
    }
    if let Some(m) = &app.message {
        lines.push(Line::raw(""));
        lines.push(Line::raw(m.clone()));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn failure_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for r in app
        .state
        .results
        .iter()
        .filter(|r| r.failed || r.unreachable)
    {
        lines.push(Line::raw(format!("✗ {}: {}", r.host, r.task)));
        if let Some(m) = &r.msg {
            lines.push(Line::styled(format!("    {m}"), dim()));
        }
    }
    for f in app.state.event_failures() {
        lines.push(Line::raw(format!("✗ {}: {}", f.key, f.detail)));
    }
    for l in app
        .last_tail
        .lines()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        lines.push(Line::styled(format!("  {l}"), dim()));
    }
    let hits = app.hints();
    if !hits.is_empty() {
        lines.push(Line::raw(""));
    }
    for h in hits {
        lines.push(Line::styled(format!("Cause: {}", h.cause), bold()));
        lines.push(Line::raw(format!("Fix:   {}", h.fix)));
        if let Some(link) = &h.link {
            lines.push(Line::styled(format!("See:   {link}"), dim()));
        }
    }
    lines
}

fn check_failed(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = vec![
        Line::styled("Check failed. Nothing was changed.", bold()),
        Line::raw(""),
    ];
    if let Some(m) = &app.message {
        lines.push(Line::styled(m.clone(), bold()));
        lines.push(Line::raw(""));
    }
    lines.extend(failure_lines(app));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn result(app: &App, f: &mut Frame, area: Rect) {
    let c = app.command();
    let ok = app.error.is_none() && app.last_exit == Some(0) && !app.state.any_failed();
    let mut lines = Vec::new();
    if ok {
        lines.push(Line::styled(
            summary_for(c.done.as_deref(), &app.state),
            bold(),
        ));
        if let Some(n) = &app.next_step {
            lines.push(Line::raw(""));
            lines.push(Line::raw(n.clone()));
        }
    } else {
        lines.push(Line::styled("The run failed.", bold()));
        if let Some(m) = &app.message {
            lines.push(Line::styled(m.clone(), bold()));
        }
        if let Some(e) = &app.error {
            lines.push(Line::raw(e.clone()));
        }
        lines.push(Line::raw(""));
        lines.extend(failure_lines(app));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}
