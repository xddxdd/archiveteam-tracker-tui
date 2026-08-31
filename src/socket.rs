//! Minimal client for socket.io 0.9 / protocol 1, which the ArchiveTeam
//! tracker still speaks (see /js/socket.io.js on the tracker page).
//!
//! Flow, mirroring the browser client:
//!   1. HTTP GET  https://<log_host>/socket.io/1/  ->  "sid:heartbeat:close:transports"
//!   2. WebSocket wss://<log_host>/socket.io/1/websocket/<sid>
//!   3. Send "1::<namespace>" to subscribe to the project's log channel.
//!   4. Receive "5::<ns>:{\"name\":\"log_message\",\"args\":[\"<json>\"]}" events.
//!   5. Echo "2::" back whenever the server sends a heartbeat.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::client::IntoClientRequest;

use crate::api::{self, RawMsg};
use crate::state::AppState;

type Ws = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

pub struct SocketCfg {
    pub log_host: String,
    pub project: String,
}

/// Reconnect loop with exponential backoff. A connection that lived for a
/// while resets the wait, mirroring the browser client's behavior.
pub fn run(cfg: SocketCfg, state: Arc<Mutex<AppState>>, shutdown: Arc<AtomicBool>) {
    let channel = format!("/{}-log", cfg.project);
    let mut backoff = 2u64;
    while !shutdown.load(Ordering::Relaxed) {
        let started = Instant::now();
        match run_once(&cfg, &channel, &state, &shutdown) {
            Ok(()) => return, // shutdown requested
            Err(e) => {
                set_status(&state, false, &format!("reconnecting: {e}"));
                let wait = if started.elapsed() > Duration::from_secs(60) {
                    2
                } else {
                    backoff
                };
                backoff = (backoff * 2).min(60);
                if !sleep_interruptible(&shutdown, wait) {
                    return;
                }
            }
        }
    }
}

/// One connection attempt; Ok(()) means shutdown was requested.
fn run_once(
    cfg: &SocketCfg,
    channel: &str,
    state: &Arc<Mutex<AppState>>,
    shutdown: &AtomicBool,
) -> Result<(), String> {
    set_status(state, false, "handshaking");
    let hs = handshake(&cfg.log_host)?;
    let mut socket = connect(cfg, &hs, state)?;

    // the 0.9 server only accepts namespace joins after it has sent the root
    // "1::" connect ack, so wait for that first
    await_root_ack(&mut socket, shutdown)?;
    send(&mut socket, &format!("1::{channel}"))?;

    set_status(state, false, "waiting for events");
    let dead_after = Duration::from_secs(hs.heartbeat + hs.close + 30);
    event_loop(&mut socket, channel, state, shutdown, dead_after)
}

struct Handshake {
    sid: String,
    heartbeat: u64,
    close: u64,
}

fn handshake(log_host: &str) -> Result<Handshake, String> {
    let url = format!("https://{log_host}/socket.io/1/");
    let body = api::http_get(&url, Duration::from_secs(15))?;
    let mut parts = body.trim().split(':');
    let sid = parts.next().unwrap_or_default().to_string();
    let heartbeat = parts.next().and_then(|v| v.parse().ok()).unwrap_or(60u64);
    let close = parts.next().and_then(|v| v.parse().ok()).unwrap_or(60u64);
    let transports = parts.next().unwrap_or_default();
    if sid.is_empty() || !transports.split(',').any(|t| t == "websocket") {
        return Err(format!("bad handshake response: {body}"));
    }
    Ok(Handshake {
        sid,
        heartbeat,
        close,
    })
}

fn connect(cfg: &SocketCfg, hs: &Handshake, state: &Arc<Mutex<AppState>>) -> Result<Ws, String> {
    set_status(state, false, "connecting websocket");

    let (host, port) = split_host_port(&cfg.log_host)?;
    let tcp = TcpStream::connect((host.as_str(), port))
        .map_err(|e| format!("connect {host}:{port}: {e}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(135)))
        .map_err(|e| format!("set timeout: {e}"))?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let ws_url = format!("wss://{}/socket.io/1/websocket/{}", cfg.log_host, hs.sid);
    let mut req = ws_url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("request: {e}"))?;
    req.headers_mut().insert(
        "Origin",
        tungstenite::http::HeaderValue::from_str(&format!("https://{}", cfg.log_host))
            .map_err(|e| format!("origin: {e}"))?,
    );
    let (socket, _resp) =
        tungstenite::client_tls(req, tcp).map_err(|e| format!("websocket handshake: {e}"))?;
    Ok(socket)
}

/// Wait for the root-namespace "1::" connect ack.
fn await_root_ack(socket: &mut Ws, shutdown: &AtomicBool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return Ok(());
        }
        match read_text(socket)? {
            Some(text) => {
                for frame in decode_payload(&text) {
                    match parse_packet(&frame) {
                        // empty endpoint = root namespace connect ack
                        Some(Packet::Connect(ep)) if ep.is_empty() => return Ok(()),
                        Some(Packet::Heartbeat) => send(socket, "2::")?,
                        _ => {}
                    }
                }
            }
            None if Instant::now() > deadline => return Err("no root connect ack".to_string()),
            None => {}
        }
    }
}

fn event_loop(
    socket: &mut Ws,
    channel: &str,
    state: &Arc<Mutex<AppState>>,
    shutdown: &AtomicBool,
    dead_after: Duration,
) -> Result<(), String> {
    let mut last_msg = Instant::now();
    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return Ok(());
        }
        match read_text(socket)? {
            Some(text) => {
                last_msg = Instant::now();
                for frame in decode_payload(&text) {
                    handle_frame(socket, channel, &frame, state)?;
                }
            }
            None if last_msg.elapsed() > dead_after => {
                return Err("server stopped sending heartbeats".to_string());
            }
            None => {}
        }
    }
}

/// socket.io 0.9 / protocol 1 packet types. Wire format per packet:
/// "type:id[+]:endpoint?:data?" — only the subset the tracker emits is
/// modeled; everything else parses to None and is ignored.
#[derive(Debug, PartialEq)]
enum Packet {
    Disconnect(String),
    Connect(String),
    Heartbeat,
    Event(String, String), // (endpoint, raw data)
}

fn parse_packet(frame: &str) -> Option<Packet> {
    let mut parts = frame.splitn(4, ':');
    let typ = parts.next()?;
    let _id = parts.next().unwrap_or("");
    let endpoint = parts.next().unwrap_or("").to_string();
    let data = parts.next().unwrap_or("").to_string();
    match typ {
        "0" => Some(Packet::Disconnect(endpoint)),
        "1" => Some(Packet::Connect(endpoint)),
        "2" => Some(Packet::Heartbeat),
        "5" => Some(Packet::Event(endpoint, data)),
        _ => None,
    }
}

fn handle_frame(
    socket: &mut Ws,
    channel: &str,
    frame: &str,
    state: &Arc<Mutex<AppState>>,
) -> Result<(), String> {
    match parse_packet(frame) {
        Some(Packet::Heartbeat) => send(socket, "2::")?,
        Some(Packet::Connect(ep)) if ep == channel => set_status(state, true, "live"),
        Some(Packet::Disconnect(ep)) => return Err(format!("server disconnected {ep}")),
        Some(Packet::Event(ep, data)) if ep == channel => {
            if let Some(msg) = parse_log_message(&data) {
                state.lock().unwrap().apply_event(&msg);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Read the next websocket text message, skipping control frames.
/// Ok(None) means the read timed out (idle tick, not an error).
fn read_text(socket: &mut Ws) -> Result<Option<String>, String> {
    loop {
        match socket.read() {
            Ok(msg) if msg.is_text() || msg.is_binary() => {
                let text = msg.into_text().map_err(|e| format!("{e}"))?;
                return Ok(Some(text.to_string()));
            }
            Ok(_) => continue,
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
            {
                return Ok(None);
            }
            Err(tungstenite::Error::ConnectionClosed) | Err(tungstenite::Error::AlreadyClosed) => {
                return Err("connection closed".to_string());
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }
}

/// tungstenite's write() only queues; every message must be flushed to reach
/// the wire, or the server silently ignores it
fn send(socket: &mut Ws, text: &str) -> Result<(), String> {
    socket
        .write(tungstenite::Message::Text(text.into()))
        .map_err(|e| format!("send: {e}"))?;
    socket.flush().map_err(|e| format!("send: {e}"))
}

fn set_status(state: &Arc<Mutex<AppState>>, connected: bool, status: &str) {
    let mut s = state.lock().unwrap();
    s.connected = connected;
    s.status = status.to_string();
}

fn sleep_interruptible(shutdown: &AtomicBool, secs: u64) -> bool {
    for _ in 0..(secs * 2) {
        if shutdown.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    true
}

/// Extract the log_message payload: {"name":"log_message","args":["<json>"]}
fn parse_log_message(data: &str) -> Option<RawMsg> {
    let v: Value = serde_json::from_str(data).ok()?;
    if v.get("name")?.as_str()? != "log_message" {
        return None;
    }
    let payload = v.get("args")?.get(0)?.as_str()?;
    serde_json::from_str(payload).ok()
}

/// A single websocket text frame may carry several socket.io packets glued
/// together as \ufffd<len>\ufffd<packet> — same decoding as the 0.9 client.
fn decode_payload(s: &str) -> Vec<String> {
    if !s.starts_with('\u{fffd}') {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut parts = s.split('\u{fffd}');
    parts.next(); // leading empty
    while let (Some(_len), Some(frame)) = (parts.next(), parts.next()) {
        if !frame.is_empty() {
            out.push(frame.to_string());
        }
    }
    out
}

fn split_host_port(hostport: &str) -> Result<(String, u16), String> {
    let (host, port) = hostport
        .rsplit_once(':')
        .ok_or_else(|| format!("bad host:port {hostport}"))?;
    let port = port
        .parse()
        .map_err(|_| format!("bad port in {hostport}"))?;
    Ok((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_single() {
        assert_eq!(decode_payload("1::"), vec!["1::"]);
    }

    #[test]
    fn decode_multi() {
        let s = "\u{fffd}4\u{fffd}1::\u{fffd}3\u{fffd}2::";
        assert_eq!(decode_payload(s), vec!["1::", "2::"]);
    }

    #[test]
    fn parse_packet_types() {
        assert_eq!(parse_packet("0::"), Some(Packet::Disconnect(String::new())));
        assert_eq!(parse_packet("1::"), Some(Packet::Connect(String::new())));
        assert_eq!(
            parse_packet("1::/sinavideo-log"),
            Some(Packet::Connect("/sinavideo-log".into()))
        );
        assert_eq!(parse_packet("2::"), Some(Packet::Heartbeat));
        assert_eq!(
            parse_packet("5::/sinavideo-log:{\"name\":\"x\"}"),
            Some(Packet::Event(
                "/sinavideo-log".into(),
                "{\"name\":\"x\"}".into()
            ))
        );
        assert_eq!(parse_packet("8::"), None); // noop, unused
        assert_eq!(parse_packet("garbage"), None);
    }

    #[test]
    fn parse_event() {
        let inner = serde_json::json!({"downloader":"foo","item":"x:1","bytes":123,"ts":1.5});
        let data = serde_json::json!({"name":"log_message","args":[inner.to_string()]}).to_string();
        let msg = parse_log_message(&data).unwrap();
        assert_eq!(msg.downloader.as_deref(), Some("foo"));
        assert_eq!(msg.bytes, Some(123.0));
    }
}
