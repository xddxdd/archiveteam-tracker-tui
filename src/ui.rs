use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::prelude::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, Gauge, Paragraph, Row, Table};
use ratatui::Frame;

use crate::format::{compact, duration, human_bytes, speed};
use crate::state::{AppState, DlStats, LogLine};

pub fn draw(f: &mut Frame, st: &AppState) {
    let [top, main, bottom] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)]).areas(main);

    draw_top(f, top, st);
    draw_leaderboard(f, left, st);
    draw_log(f, right, st);
    draw_bottom(f, bottom, st);
}

fn draw_top(f: &mut Frame, area: Rect, st: &AppState) {
    let status = if st.connected {
        Span::styled(" ● live", Style::new().fg(Color::Green))
    } else {
        Span::styled(" ○ offline", Style::new().fg(Color::Red))
    };
    let block = Block::bordered()
        .title(format!(" {} · ArchiveTeam tracker ", st.project))
        .title_top(Line::from(status).right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [gauge_area, counts_area, rates_area, up_area] =
        Layout::vertical([Constraint::Length(1); 4]).areas(inner);

    render_gauge(f, gauge_area, st);
    f.render_widget(Paragraph::new(counts_line(st)), counts_area);
    f.render_widget(Paragraph::new(rates_line(st)), rates_area);
    f.render_widget(Paragraph::new(uptime_line(st)), up_area);
}

fn render_gauge(f: &mut Frame, area: Rect, st: &AppState) {
    let total = st.total_items();
    let ratio = if total > 0 {
        st.done as f64 / total as f64
    } else {
        0.0
    };
    f.render_widget(
        Gauge::default()
            .ratio(ratio.clamp(0.0, 1.0))
            .gauge_style(Color::Cyan)
            .label(format!(
                "done {} of {} items ({:.1}%)",
                compact(st.done),
                compact(total),
                ratio * 100.0
            )),
        area,
    );
}

fn counts_line(st: &AppState) -> Line<'static> {
    let mut spans = field("done ", compact(st.done));
    spans.extend(field(" out ", compact(st.out)));
    spans.extend(field(" todo ", compact(st.todo)));
    spans.extend(field(" backlog ", compact(st.done + st.todo)));
    Line::from(spans)
}

fn rates_line(st: &AppState) -> Line<'static> {
    let (b60, _) = st.rates_over(60);
    let (b600, i600) = st.rates_over(600);
    let rate = |v: f64| {
        if st.has_rate_data() {
            speed(v)
        } else {
            "…".to_string()
        }
    };

    let mut spans = field("data ", human_bytes(st.total_bytes));
    spans.extend(field("  ↓ ", rate(b60)));
    spans.extend(field(" (60s)  avg ", rate(b600)));
    spans.push(Span::styled(" (10m)", dim()));
    if st.has_rate_data() {
        spans.extend(field("  items ", format!("{}/min", compact(i600 as u64))));
    }
    Line::from(spans)
}

fn uptime_line(st: &AppState) -> Line<'static> {
    let mut spans = field("up ", duration(st.started.elapsed()));
    spans.extend(field("  events ", compact(st.event_count)));
    if let Some(rcr) = st.rcr {
        spans.extend(field("  reclaim ", format!("{:.1}%", rcr * 100.0)));
    }
    if let Some(v) = st.queue("todo:backfeed") {
        spans.extend(field("  backfeed ", compact(v)));
    }
    if let Some(e) = &st.error {
        spans.push(Span::styled(
            format!("  ⚠ {e}"),
            Style::new().fg(Color::Red),
        ));
    }
    Line::from(spans)
}

fn draw_leaderboard(f: &mut Frame, area: Rect, st: &AppState) {
    let block = Block::bordered().title(format!(" Top {} downloaders ", st.top.max(1)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if !st.loaded {
        f.render_widget(Paragraph::new("loading…"), inner);
        return;
    }
    if inner.width < 8 || inner.height < 2 {
        return;
    }

    let rows = leaderboard_rows(st, inner.height as usize);
    f.render_widget(Table::new(rows, TABLE_WIDTHS), inner);
}

fn leaderboard_rows(st: &AppState, height: usize) -> Vec<Row<'static>> {
    let mut sorted: Vec<(&String, &DlStats)> = st.downloaders.iter().collect();
    sorted.sort_unstable_by_key(|(_, d)| std::cmp::Reverse(d.bytes));

    let top_n = st.top.max(1);
    let nick = st.nickname.as_ref();
    let my_rank = nick.and_then(|n| {
        sorted
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(n))
    });

    let mut rows = vec![leaderboard_header()];
    for (i, (name, d)) in sorted.iter().take(top_n).enumerate() {
        rows.push(dl_row(i + 1, name, d, nick));
    }

    // top N is always shown in full; the own-nickname row is appended below it
    // (with a "…" separator) whenever the panel has room left
    if let Some(n) = nick {
        let data_rows = rows.len() - 1;
        match my_rank {
            Some(r) if r >= top_n && data_rows + 2 <= height => {
                rows.push(ellipsis_row());
                let (name, d) = sorted[r];
                rows.push(dl_row(r + 1, name, d, Some(n)));
            }
            None if data_rows < height => rows.push(unseen_row(n)),
            _ => {}
        }
    }
    rows
}

fn leaderboard_header() -> Row<'static> {
    Row::new(vec![
        Cell::from("#").style(dim()),
        Cell::from("downloader").style(dim()),
        right("size").style(dim()),
        right("items").style(dim()),
    ])
}

const TABLE_WIDTHS: [Constraint; 4] = [
    Constraint::Length(4),
    Constraint::Min(6),
    Constraint::Length(11),
    Constraint::Length(8),
];

const LOG_WIDTHS: [Constraint; 4] = [
    Constraint::Length(8),
    Constraint::Length(16),
    Constraint::Min(10),
    Constraint::Length(10),
];

fn dl_row(rank: usize, name: &str, d: &DlStats, nick: Option<&String>) -> Row<'static> {
    let is_me = nick.is_some_and(|n| n.eq_ignore_ascii_case(name));
    let style = if is_me { me() } else { Style::new() };
    Row::new(vec![
        Cell::from(rank.to_string()).style(style),
        Cell::from(if is_me {
            format!("{name} (you)")
        } else {
            name.to_string()
        })
        .style(style),
        right(human_bytes(d.bytes)).style(style),
        right(compact(d.items)).style(style),
    ])
}

fn ellipsis_row() -> Row<'static> {
    Row::new(vec![
        Cell::from("…").style(dim()),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
    ])
}

fn unseen_row(nick: &str) -> Row<'static> {
    Row::new(vec![
        Cell::from("—").style(dim()),
        Cell::from(Span::styled(format!("{nick} (you)"), me())),
        right("not seen").style(dim()),
        Cell::from(""),
    ])
}

fn draw_log(f: &mut Frame, area: Rect, st: &AppState) {
    let title = if st.log_offset == 0 {
        " Live log (newest first) ".to_string()
    } else {
        format!(" Live log (+{} older) ", st.log_offset)
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 16 || inner.height < 1 {
        return;
    }

    let mut rows = vec![log_header()];
    rows.extend(
        visible_lines(st, inner.height as usize)
            .iter()
            .map(|l| log_row(l, st.nickname.as_ref())),
    );
    f.render_widget(Table::new(rows, LOG_WIDTHS), inner);
}

// newest lines on top: skip `log_offset` newest entries, then fill the panel
// with successively older ones
fn visible_lines(st: &AppState, height: usize) -> Vec<&LogLine> {
    st.log
        .iter()
        .rev()
        .skip(st.log_offset.min(st.log.len()))
        .take(height)
        .collect()
}

fn log_header() -> Row<'static> {
    Row::new(vec![
        Cell::from("time").style(dim()),
        Cell::from("downloader").style(dim()),
        Cell::from("item").style(dim()),
        right("size").style(dim()),
    ])
}

fn log_row(l: &LogLine, nick: Option<&String>) -> Row<'static> {
    let row_style = if l.dup {
        Style::new().fg(Color::DarkGray)
    } else {
        Style::new()
    };
    Row::new(vec![
        Cell::from(time_str(l.ts)).style(row_style),
        Cell::from(l.downloader.clone()).style(downloader_style(l, nick)),
        Cell::from(item_text(l)).style(row_style),
        right(human_bytes(l.bytes)).style(row_style),
    ])
}

fn time_str(ts: f64) -> String {
    if ts <= 0.0 {
        return "--:--:--".to_string();
    }
    chrono::DateTime::from_timestamp(ts as i64, ((ts - ts.floor()) * 1e9) as u32)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

fn downloader_style(l: &LogLine, nick: Option<&String>) -> Style {
    if nick.is_some_and(|n| n.eq_ignore_ascii_case(&l.downloader)) {
        me()
    } else if l.user_agent.ends_with("Standalone") {
        Style::new().fg(Color::Cyan)
    } else if l.user_agent.ends_with("Warrior") {
        Style::new().fg(Color::Green)
    } else {
        Style::new().fg(Color::Yellow)
    }
}

fn item_text(l: &LogLine) -> String {
    if l.n_items > 1 {
        format!("{} × {}", l.n_items, l.item)
    } else {
        l.item.clone()
    }
}

fn draw_bottom(f: &mut Frame, area: Rect, st: &AppState) {
    let [keys_area, status_area] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(30)]).areas(area);
    f.render_widget(Paragraph::new(keys_line()), keys_area);
    f.render_widget(
        Paragraph::new(Line::from(status_span(st)).right_aligned()),
        status_area,
    );
}

const KEY_HINTS: [(&str, &str); 5] = [
    (" q", " quit  "),
    ("↑/↓ j/k", " scroll  "),
    ("PgUp/PgDn", " page  "),
    ("f", " follow  "),
    ("r", " refresh"),
];

fn keys_line() -> Line<'static> {
    let mut spans = Vec::new();
    for (key, hint) in KEY_HINTS {
        spans.push(Span::styled(key, Style::new().fg(Color::Cyan).bold()));
        spans.push(Span::styled(hint, dim()));
    }
    Line::from(spans)
}

fn status_span(st: &AppState) -> Span<'static> {
    let color = if st.error.is_some() {
        Color::Red
    } else {
        Color::DarkGray
    };
    Span::styled(format!(" {} ", st.status), Style::new().fg(color))
}

fn field(label: &str, val: impl Into<String>) -> Vec<Span<'static>> {
    vec![Span::styled(label.to_string(), dim()), value(val)]
}

fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

fn right(s: impl Into<String>) -> Cell<'static> {
    Cell::from(Text::from(s.into()).alignment(Alignment::Right))
}

fn me() -> Style {
    Style::new().fg(Color::Yellow).bold()
}

fn value(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(Color::White).bold())
}
