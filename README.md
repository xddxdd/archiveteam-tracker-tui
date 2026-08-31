# archiveteam-tracker-tui

Terminal UI for monitoring [ArchiveTeam tracker](https://tracker.archiveteam.org/) projects, written in Rust with ratatui.

It talks to the tracker exactly like the web dashboard does — one `stats.json` fetch plus a socket.io log subscription.

```
nix run github:you/archiveteam-tracker-tui -- sinavideo --nickname yournick
```

Or from a checkout:

```
nix run . -- sinavideo --nickname yournick
nix develop   # dev shell: cargo, rustfmt, clippy, rust-analyzer
```

## Usage

```
attui [OPTIONS] <PROJECT>
```

- `PROJECT` — tracker project ID, e.g. `sinavideo` (any project on tracker.archiveteam.org works)
- `-n, --nickname <NICK>` — your downloader name; always shown on the leaderboard regardless of rank, highlighted in yellow, defaults to your `$USER`
- `-t, --top <N>` — leaderboard size (default 20)
- `--api-base`, `--log-host` — override endpoints for other tracker deployments

## Layout

- **Top**: items done/out/todo gauge, total data archived, live bandwidth (60s and 10m windows), items/min, reclaim rate
- **Left**: top downloaders by volume; your own row is always visible (rank number included even at #1300+)
- **Right**: live log of finished items, newest at the top (like the web dashboard), older entries pushed down; your nick is highlighted, warrior/standalone clients colored. `j/k` scroll deeper into history, `f` snaps back to newest.

## Keys

`q` quit · `↑/↓ j/k` scroll log · `PgUp/PgDn` page · `f` re-follow newest · `g/G` newest/oldest · `r` force stats refresh

## How it connects

The tracker's socket.io is ancient (0.9 / protocol 1), so this implements that protocol directly: HTTP handshake for a session id, a `wss` websocket, a namespace join (`1::/{project}-log`), heartbeat echo (`2::`), and `log_message` event parsing. Server-sent counters are used for totals; per-downloader bytes accumulate from events, refreshed against `stats.json` every 5 minutes.
