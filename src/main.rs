mod api;
mod format;
mod socket;
mod state;
mod ui;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;

use state::AppState;

#[derive(Parser)]
#[command(
    name = "attui",
    about = "TUI for monitoring an ArchiveTeam tracker project"
)]
struct Args {
    /// Project ID, e.g. sinavideo (see https://tracker.archiveteam.org/)
    project: String,

    /// Your downloader nickname; always shown on the leaderboard regardless
    /// of position. Defaults to $USER.
    #[arg(short, long)]
    nickname: Option<String>,

    /// Number of top downloaders to show
    #[arg(short, long, default_value_t = 20)]
    top: usize,

    /// Tracker stats API base URL
    #[arg(long, default_value = "https://v1.api.tracker.archiveteam.org")]
    api_base: String,

    /// Tracker log host (TLS websocket + recent log), host:port
    #[arg(long, default_value = "tracker.archiveteam.org:8081")]
    log_host: String,
}

/// Background thread: fetch stats.json (initial + periodic refresh) and the
/// recent-log bootstrap, exactly like the web dashboard does on page load.
fn run_fetcher(
    args: Arc<Args>,
    state: Arc<Mutex<AppState>>,
    refresh: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) {
    let mut first = true;
    loop {
        match api::fetch_stats(&args.api_base, &args.project) {
            Ok(stats) => {
                state.lock().unwrap().apply_stats(&stats);
                if first {
                    first = false;
                    if let Ok(msgs) = api::fetch_recent(&args.log_host, &args.project) {
                        let mut s = state.lock().unwrap();
                        for m in msgs {
                            s.apply_history(&m);
                        }
                    }
                }
            }
            Err(e) => {
                state.lock().unwrap().error = Some(e);
            }
        }

        // poll for shutdown / manual refresh while waiting for the next cycle
        let interval = if first { 15 } else { 300 };
        let mut waited = 0;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            if refresh.swap(false, Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
            waited += 1;
            if waited >= interval {
                break;
            }
        }
    }
}

fn run_tui(
    terminal: &mut ratatui::DefaultTerminal,
    state: &Arc<Mutex<AppState>>,
    refresh: &Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        {
            let s = state.lock().unwrap();
            terminal.draw(|f| ui::draw(f, &s))?;
        }

        if crossterm::event::poll(Duration::from_millis(250))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                use crossterm::event::{KeyCode, KeyModifiers};
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }
                let page = 20usize;
                let mut s = state.lock().unwrap();
                let max_offset = s.log.len().saturating_sub(1);
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q') | KeyCode::Esc, _) => return Ok(()),
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(()),
                    (KeyCode::Char('r'), _) => refresh.store(true, Ordering::Relaxed),
                    // newest lines render at the top; scrolling down moves deeper
                    // into history
                    (KeyCode::Char('f') | KeyCode::Char('g') | KeyCode::Home, _) => {
                        s.log_offset = 0;
                    }
                    (KeyCode::End | KeyCode::Char('G'), _) => {
                        s.log_offset = max_offset;
                    }
                    (KeyCode::Down | KeyCode::Char('j'), _) => {
                        s.log_offset = (s.log_offset + 1).min(max_offset);
                    }
                    (KeyCode::Up | KeyCode::Char('k'), _) => {
                        s.log_offset = s.log_offset.saturating_sub(1);
                    }
                    (KeyCode::PageDown, _) => {
                        s.log_offset = (s.log_offset + page).min(max_offset);
                    }
                    (KeyCode::PageUp, _) => {
                        s.log_offset = s.log_offset.saturating_sub(page);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Arc::new(Args::parse());

    // empty string disables the personal row
    let nickname = args
        .nickname
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| std::env::var("USER").ok().filter(|n| !n.is_empty()));

    let state = Arc::new(Mutex::new(AppState::new(&args.project, nickname, args.top)));
    let shutdown = Arc::new(AtomicBool::new(false));
    let refresh = Arc::new(AtomicBool::new(false));

    let fetcher_state = state.clone();
    let fetcher_refresh = refresh.clone();
    let fetcher_shutdown = shutdown.clone();
    let fetcher_args = args.clone();
    std::thread::spawn(move || {
        run_fetcher(
            fetcher_args,
            fetcher_state,
            fetcher_refresh,
            fetcher_shutdown,
        )
    });

    let socket_state = state.clone();
    let socket_shutdown = shutdown.clone();
    let socket_args = args.clone();
    std::thread::spawn(move || {
        socket::run(
            socket::SocketCfg {
                log_host: socket_args.log_host.clone(),
                project: socket_args.project.clone(),
            },
            socket_state,
            socket_shutdown,
        )
    });

    let mut terminal = ratatui::init();
    let result = run_tui(&mut terminal, &state, &refresh);
    ratatui::restore();

    shutdown.store(true, Ordering::Relaxed);
    result
}
