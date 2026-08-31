# AGENTS.md — archiveteam-tracker-tui

## 项目概述

ArchiveTeam tracker 的终端监控界面（Rust + ratatui），通过官方 tracker 的真实数据通道（stats.json + socket.io 0.9 日志订阅）获取数据。

## 开发环境

- NixOS，使用 flake.nix 管理环境
- `nix develop` — devShell（cargo / rustc / rustfmt / clippy / rust-analyzer）
- `nix build .#` — 构建发布包（`result/bin/attui`）
- 常规流程：`cargo fmt && cargo clippy --all-targets && cargo test && cargo build`

## 目录结构

```
src/
├── main.rs    — clap 参数、TUI 主循环、后台线程编排（fetcher + socket）
├── state.rs   — AppState：stats.json 基线 + 实时事件累加、速率窗口、日志缓冲
├── api.rs     — HTTP 抓取（stats.json / recent log）与 serde 反序列化（数值字段兼容 int/float）
├── socket.rs  — socket.io 0.9 / protocol 1 客户端（握手、wss、namespace join、心跳、重连）
├── ui.rs      — ratatui 渲染（顶栏统计 / 左侧排行榜 / 右侧实时日志）
└── format.rs  — IEC 字节、紧凑计数、速率、时长格式化
```

## 关键协议事实（勿凭直觉改动）

- tracker.archiveteam.org 的 socket.io 是 **0.9.6 / protocol 1**，与现代 socket.io 客户端（EIO 3/4）不兼容，因此 socket.rs 手工实现协议。
- 连接流程：`GET https://{log_host}/socket.io/1/` 取 sid → `wss://{log_host}/socket.io/1/websocket/{sid}` → 等到根命名空间的 `1::` ack 后发送 `1::/{project}-log` 加入频道 → 监听 `5::/{ns}:{"name":"log_message","args":["<json>"]}` → 收到 `2::` 必须回发 `2::`。
- 事件中 `counts` 是服务器权威计数；`downloader_bytes`/`total_bytes` 是网页同款做法的本地累加，5 分钟用 stats.json 全量校准一次。
- `tungstenite::WebSocket::write()` 只入队不落盘，**每次 write 后必须 `flush()`**，否则加入消息永远发不出去。

## 约定

- 代码注释遵循全局规则：只为复杂逻辑 / hacky workaround / 非直观业务规则写注释。
- 排行榜按 bytes 降序；nickname 大小写不敏感匹配；自己的行加 "(you)" 后缀并高亮。
- 速率窗口用样本实际时间跨度做除数（clamp 到窗口长度），避免启动初期速率虚低。
- UI 测试方式：`tmux new-session -d -x 140 -y 42 -s att "./result/bin/attui <project>"` 后 `tmux capture-pane -t att -p` 查看渲染结果（本仓库无交互终端时用此法冒烟测试）。
