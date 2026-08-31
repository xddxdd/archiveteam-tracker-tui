use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use crate::api::{RawMsg, StatsJson};

pub struct DlStats {
    pub bytes: u64,
    pub items: u64,
}

pub struct LogLine {
    pub ts: f64,
    pub downloader: String,
    pub item: String,
    pub n_items: usize,
    pub dup: bool,
    pub bytes: u64,
    pub user_agent: String,
}

struct RatePoint {
    t: Instant,
    bytes: u64,
    items: u64,
}

pub struct AppState {
    pub project: String,
    pub nickname: Option<String>,
    pub top: usize,

    pub connected: bool,
    pub status: String,
    pub error: Option<String>,
    pub loaded: bool,

    pub started: Instant,
    pub done: u64,
    pub out: u64,
    pub todo: u64,
    pub rcr: Option<f64>,
    pub rcsr: Option<f64>,
    pub irsr: Option<f64>,
    pub queues: Vec<(String, u64)>,

    pub total_bytes: u64,
    pub event_count: u64,
    pub downloaders: HashMap<String, DlStats>,
    pub log: VecDeque<LogLine>,
    rates: VecDeque<RatePoint>,

    /// 0 = newest lines at the top of the log panel; n = skip the n newest
    /// lines (view is scrolled n lines deeper into history)
    pub log_offset: usize,
}

impl AppState {
    pub fn new(project: &str, nickname: Option<String>, top: usize) -> Self {
        Self {
            project: project.to_string(),
            nickname,
            top,
            connected: false,
            status: "connecting".to_string(),
            error: None,
            loaded: false,
            started: Instant::now(),
            done: 0,
            out: 0,
            todo: 0,
            rcr: None,
            rcsr: None,
            irsr: None,
            queues: Vec::new(),
            total_bytes: 0,
            event_count: 0,
            downloaders: HashMap::new(),
            log: VecDeque::with_capacity(600),
            rates: VecDeque::new(),
            log_offset: 0,
        }
    }

    pub fn total_items(&self) -> u64 {
        self.done + self.out + self.todo
    }

    pub fn queue(&self, key: &str) -> Option<u64> {
        self.queues.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    pub fn apply_stats(&mut self, s: &StatsJson) {
        self.done = s.counts.done.unwrap_or(self.done);
        self.out = s.counts.out.unwrap_or(self.out);
        self.todo = s.counts.todo.unwrap_or(self.todo);
        self.rcr = s.counts.rcr.or(self.rcr);
        self.rcsr = s.counts.rcsr.or(self.rcsr);
        self.irsr = s.counts.irsr.or(self.irsr);

        self.downloaders.clear();
        for name in &s.downloaders {
            let bytes = s
                .downloader_bytes
                .as_ref()
                .and_then(|m| m.get(name))
                .map(|v| *v as u64)
                .unwrap_or(0);
            let items = s
                .downloader_count
                .as_ref()
                .and_then(|m| m.get(name))
                .copied()
                .unwrap_or(0);
            self.downloaders
                .insert(name.clone(), DlStats { bytes, items });
        }
        self.total_bytes = s
            .domain_bytes
            .as_ref()
            .map(|m| m.values().map(|v| *v as u64).sum())
            .unwrap_or(0);
        self.loaded = true;
        self.error = None;
    }

    /// Apply one live log_message event, mirroring the web dashboard's
    /// updateStats(): counts come from the server, per-downloader bytes and
    /// totals are accumulated locally. Rates/event counters are only touched
    /// for live (socket) events, not the recent-log bootstrap.
    pub fn apply_event(&mut self, msg: &RawMsg) {
        self.apply_msg(msg, true);
    }

    pub fn apply_history(&mut self, msg: &RawMsg) {
        self.apply_msg(msg, false);
    }

    fn apply_msg(&mut self, msg: &RawMsg, live: bool) {
        if let Some(c) = &msg.counts {
            if let Some(v) = c.done {
                self.done = v;
            }
            if let Some(v) = c.out {
                self.out = v;
            }
            if let Some(v) = c.todo {
                self.todo = v;
            }
            if let Some(v) = c.rcr {
                self.rcr = Some(v);
            }
            if let Some(v) = c.rcsr {
                self.rcsr = Some(v);
            }
            if let Some(v) = c.irsr {
                self.irsr = Some(v);
            }
        }
        if let Some(queues) = msg.stats.as_ref().and_then(|s| s.queues.as_ref()) {
            let mut v: Vec<(String, u64)> = queues.iter().map(|(k, x)| (k.clone(), *x)).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            self.queues = v;
        }

        let Some(downloader) = msg.downloader.clone() else {
            return;
        };

        let domain_sum: u64 = msg
            .domain_bytes
            .as_ref()
            .map(|m| m.values().map(|v| *v as u64).sum())
            .unwrap_or(0);
        let bytes = if domain_sum > 0 {
            domain_sum
        } else {
            msg.bytes.unwrap_or(0.0) as u64
        };
        let n_items = msg
            .items
            .as_ref()
            .map(|v| v.len())
            .filter(|n| *n > 0)
            .unwrap_or(1);
        let dup = msg.is_duplicate.unwrap_or(false);

        let entry = self
            .downloaders
            .entry(downloader.clone())
            .or_insert(DlStats { bytes: 0, items: 0 });
        entry.bytes += bytes;
        if !dup {
            entry.items += n_items as u64;
        }
        self.total_bytes += bytes;
        if live {
            self.event_count += 1;
            self.rates.push_back(RatePoint {
                t: Instant::now(),
                bytes,
                items: n_items as u64,
            });
            while let Some(p) = self.rates.front() {
                if p.t.elapsed().as_secs() > 600 {
                    self.rates.pop_front();
                } else {
                    break;
                }
            }
        }

        let item = msg
            .item
            .clone()
            .or_else(|| msg.items.as_ref().and_then(|v| v.first().cloned()))
            .unwrap_or_else(|| format!("{n_items} items"));
        self.log.push_back(LogLine {
            ts: msg.ts.unwrap_or(0.0),
            downloader,
            item,
            n_items,
            dup,
            bytes,
            user_agent: msg.user_agent.clone().unwrap_or_default(),
        });
        while self.log.len() > 500 {
            self.log.pop_front();
        }
    }

    /// (bytes/s, items/min) over the last `secs` seconds. The divisor is the
    /// actual span of collected samples, capped at `secs`, so rates are
    /// meaningful immediately after startup instead of ramping up over the
    /// full window.
    pub fn rates_over(&self, secs: u64) -> (f64, f64) {
        let cutoff = std::time::Duration::from_secs(secs);
        let mut bytes = 0u64;
        let mut items = 0u64;
        let mut oldest = None;
        for p in self.rates.iter().rev() {
            if p.t.elapsed() > cutoff {
                break;
            }
            bytes += p.bytes;
            items += p.items;
            oldest = Some(p.t);
        }
        let Some(t) = oldest else {
            return (0.0, 0.0);
        };
        let span = t.elapsed().as_secs().clamp(1, secs) as f64;
        (bytes as f64 / span, items as f64 * 60.0 / span)
    }

    pub fn has_rate_data(&self) -> bool {
        !self.rates.is_empty()
    }
}
