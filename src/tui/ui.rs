//! Rendering. Pure function of `App` state.

use super::state::{App, Focus, Status};
use crate::model::selected_size;
use crate::modules::{ALL, Module};
use crate::util::fmt::{bytes, tilde};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, Gauge, List, ListItem, ListState, Paragraph, Row, Table, Wrap};

const ACCENT: Color = Color::Magenta;
const OK: Color = Color::Green;

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)]).areas(f.area());
    let [side, main] = Layout::horizontal([Constraint::Length(24), Constraint::Min(30)]).areas(body);
    draw_header(f, header, app);
    draw_sidebar(f, side, app);
    if app.module() == Module::Monitor {
        draw_monitor(f, main, app);
    } else {
        draw_module(f, main, app);
    }
    draw_footer(f, footer, app);
    if app.confirm {
        draw_confirm(f, app);
    }
    if app.help {
        draw_help(f);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let mode = if app.permanent {
        Span::styled(" DELETE PERMANENTLY ", Style::new().bg(Color::Red).fg(Color::White).bold())
    } else {
        Span::styled(" move to Trash ", Style::new().fg(OK))
    };
    let line = Line::from(vec![Span::styled(" 🧹 macbroom ", Style::new().fg(ACCENT).bold()), Span::raw("  "), mode]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let mut items = Vec::new();
    let mut selected = 0;
    let mut last_section = "";
    for (i, m) in ALL.iter().enumerate() {
        if m.section() != last_section {
            last_section = m.section();
            items.push(ListItem::new(Line::from(Span::styled(last_section, Style::new().fg(Color::DarkGray).bold()))));
        }
        if i == app.current {
            selected = items.len();
        }
        let badge = match &app.mods[i].status {
            Status::Busy(_) => " ⟳",
            _ if app.mods[i].scanned && !app.mods[i].items.is_empty() => " •",
            _ => "",
        };
        items.push(ListItem::new(format!("  {}{badge}", m.title())));
    }
    let border = if app.focus == Focus::Sidebar { ACCENT } else { Color::DarkGray };
    let list = List::new(items)
        .block(Block::bordered().border_style(Style::new().fg(border)))
        .highlight_style(Style::new().bg(ACCENT).fg(Color::Black).bold());
    let mut state = ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn status_line(app: &App) -> Line<'static> {
    let st = app.state();
    let sel = st.items.iter().filter(|i| i.selected).count();
    let status = match &st.status {
        Status::Idle if !st.scanned => Span::styled("Press Enter or s to scan", Style::new().fg(Color::Yellow)),
        Status::Idle => Span::raw(""),
        Status::Busy(t) => Span::styled(*t, Style::new().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
        Status::Done(t) => Span::styled(t.clone(), Style::new().fg(OK)),
    };
    Line::from(vec![
        status,
        Span::raw(format!("   ·   {sel} selected, {}", bytes(selected_size(&st.items)))),
    ])
}

fn draw_module(f: &mut Frame, area: Rect, app: &mut App) {
    let module = app.module();
    let [top, list] = Layout::vertical([Constraint::Length(4), Constraint::Min(3)]).areas(area);
    let mut lines = vec![
        Line::from(Span::styled(module.title(), Style::new().fg(ACCENT).bold())),
        Line::from(Span::styled(module.about(), Style::new().fg(Color::Gray))),
    ];
    if module == Module::SpaceLens {
        lines.push(Line::from(format!("📁 {}", tilde(&app.space_path))));
    }
    lines.push(status_line(app));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), top);

    let border = if app.focus == Focus::List { ACCENT } else { Color::DarkGray };
    let st = &mut app.mods[app.current];
    let rows: Vec<Row> = st
        .items
        .iter()
        .map(|i| {
            let check = if !i.is_actionable() { "   " } else if i.selected { "[✓]" } else { "[ ]" };
            let size = if i.size > 0 { bytes(i.size) } else { String::new() };
            Row::new(vec![
                Cell::from(check).style(Style::new().fg(if i.selected { OK } else { Color::DarkGray })),
                Cell::from(i.group).style(Style::new().fg(Color::Cyan)),
                Cell::from(i.label.clone()),
                Cell::from(size).style(Style::new().fg(Color::Yellow)),
                Cell::from(i.detail.clone()).style(Style::new().fg(Color::DarkGray)),
            ])
        })
        .collect();
    let widths = [Constraint::Length(3), Constraint::Length(16), Constraint::Percentage(30), Constraint::Length(9), Constraint::Min(10)];
    let table = Table::new(rows, widths)
        .header(Row::new(["", "Category", "Name", "Size", "Details"]).style(Style::new().bold().underlined()))
        .block(Block::bordered().border_style(Style::new().fg(border)))
        .row_highlight_style(Style::new().bg(Color::Rgb(60, 40, 80)).bold());
    f.render_stateful_widget(table, list, &mut st.table);
}

fn gauge<'a>(title: &'a str, ratio: f64, label: String, color: Color) -> Gauge<'a> {
    Gauge::default()
        .block(Block::bordered().title(title))
        .gauge_style(Style::new().fg(color))
        .ratio(ratio.clamp(0.0, 1.0))
        .label(label)
}

fn draw_monitor(f: &mut Frame, area: Rect, app: &App) {
    let s = &app.snapshot;
    let [g1, g2, g3, rest] = Layout::vertical([
        Constraint::Length(3), Constraint::Length(3), Constraint::Length(3), Constraint::Min(3),
    ])
    .areas(area);
    let mem = s.mem_used as f64 / s.mem_total.max(1) as f64;
    let disk_used = s.disk_total.saturating_sub(s.disk_free);
    let disk = disk_used as f64 / s.disk_total.max(1) as f64;
    let color = |r: f64| if r > 0.85 { Color::Red } else if r > 0.6 { Color::Yellow } else { OK };
    f.render_widget(gauge("CPU", s.cpu as f64 / 100.0, format!("{:.1}%", s.cpu), color(s.cpu as f64 / 100.0)), g1);
    f.render_widget(gauge("Memory", mem, format!("{} / {}", bytes(s.mem_used), bytes(s.mem_total)), color(mem)), g2);
    f.render_widget(gauge("Disk /", disk, format!("{} free", bytes(s.disk_free)), color(disk)), g3);
    let mut lines: Vec<Line> = s.lines().into_iter().map(Line::from).collect();
    let cores: String = s.cores.iter().map(|c| {
        const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        BARS[((c / 100.0 * 7.0).round() as usize).min(7)]
    }).collect();
    lines.push(Line::from(format!("Cores    {cores}")));
    f.render_widget(Paragraph::new(lines).block(Block::bordered().title("Details")), rest);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let verb = app.module().verb();
    let mut keys = vec!["↑↓ move", "Tab pane", "s scan", "space toggle", "a/n all/none"];
    let act = format!("c {}", verb.to_lowercase());
    if !verb.is_empty() { keys.push(&act); }
    if app.module() == Module::SpaceLens { keys.push("⏎ open  ⌫ up"); }
    keys.extend(["p perm", "? help", "q quit"]);
    f.render_widget(Paragraph::new(keys.join("  │  ")).style(Style::new().fg(Color::DarkGray)), area);
}

fn centered(f: &Frame, w: u16, h: u16) -> Rect {
    let a = f.area();
    let w = w.min(a.width);
    let h = h.min(a.height);
    Rect::new(a.x + (a.width - w) / 2, a.y + (a.height - h) / 2, w, h)
}

fn draw_confirm(f: &mut Frame, app: &App) {
    let st = app.state();
    let n = st.items.iter().filter(|i| i.selected).count();
    let how = if app.permanent { "PERMANENTLY" } else { "safely (Trash where applicable)" };
    let text = vec![
        Line::from(format!("{} {n} item(s), {}?", app.module().verb(), bytes(selected_size(&st.items)))).bold(),
        Line::from(format!("Files are removed {how}.")),
        Line::from(""),
        Line::from(Span::styled("y / Enter = confirm    any other key = cancel", Style::new().fg(Color::Yellow))),
    ];
    let area = centered(f, 60, 7);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(text).block(Block::bordered().title(" Confirm ").border_style(Style::new().fg(ACCENT))),
        area,
    );
}

fn draw_help(f: &mut Frame) {
    let text = [
        "↑/↓ j/k     move          PgUp/PgDn  jump",
        "Tab ←/→     switch pane   Enter      open / toggle",
        "s / r       scan/rescan   space      toggle item",
        "a / n       select all / none",
        "c x Del     run action (Clean / Uninstall / Quit / Disable / Run)",
        "p           toggle permanent delete (default: Trash)",
        "⌫           Space Lens: go up a folder",
        "q Esc       quit",
        "",
        "Tip: grant Full Disk Access to your terminal for Safari, Mail & Trash.",
    ];
    let area = centered(f, 72, 13);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(text.iter().map(|l| Line::from(*l)).collect::<Vec<_>>())
            .block(Block::bordered().title(" Help ").border_style(Style::new().fg(ACCENT))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &mut App) -> String {
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn renders_sidebar_items_and_dialogs() {
        let mut app = App::new();
        app.mods[0].items = vec![Item::files("User Cache", "com.example".into(), vec!["/x".into()], 5_000_000)];
        app.mods[0].scanned = true;
        app.mods[0].table.select(Some(0));
        let screen = render(&mut app);
        for text in ["macbroom", "Smart Care", "Uninstaller", "Space Lens", "com.example", "5.0 MB"] {
            assert!(screen.contains(text), "missing {text}");
        }
        app.confirm = true;
        assert!(render(&mut app).contains("Confirm"));
        app.confirm = false;
        app.help = true;
        assert!(render(&mut app).contains("Help"));
    }

    #[test]
    fn renders_monitor() {
        let mut app = App::new();
        app.current = ALL.iter().position(|m| *m == Module::Monitor).unwrap();
        assert!(render(&mut app).contains("Memory"));
    }
}
