# F1 AI Race Engineer & Duo Pit Wall

A Rust daemon that reads EA Sports F1 22 / 23 / 24 / 25 UDP telemetry, works out race strategy, speaks team-radio callouts, and serves a browser pit wall. A friend can use the pit wall over Tailscale to watch the race, approve AI callouts, or radio the driver themselves.

Specs: [PRD.md](PRD.md) · [UI.md](UI.md)

## Status

| Milestone | State |
| :--- | :--- |
| M1 `f1-udp` — multi-format decode, capture/replay, `f1-synth` | ✅ |
| M2 `race-state` — state, gaps, wear/fuel projections, SQLite | ✅ |
| M3 `pitwall-server` + `ui` — WebSocket/REST, 7-monitor pit wall | ✅ |
| M4 `comms` + local radio — rule engine, arbitrator, TTS → radio FX → speakers | ✅ |
| M5 Voice input (PTT, whisper, LLM Q&A) | planned |
| M6 Discord | planned |
| M7 Debrief | planned |

## Quick start

Prerequisites: Rust (stable) and Node 22+.

```sh
# 1. Build the UI once (the daemon serves ui/build).
cd ui && npm install && npm run build && cd ..

# 2. Run the daemon. It listens for UDP on :20777 and serves the pit wall on :8080.
cargo run --release -p pitwall-server
```

Open <http://localhost:8080>. The driver view is at `/driver`.

### In-game setup

Go to **Settings → Telemetry Settings** and set:

- UDP Telemetry: **On**
- UDP IP Address: the PC running the daemon (`127.0.0.1` if it's the same machine)
- UDP Port: **20777**
- UDP Send Rate: **20 Hz** or higher
- UDP Format: the game's own year (older formats also work)

### No game? Use the synthetic race

```sh
cargo run --release -p pitwall-server -- --synthetic
cargo run --release -p f1-synth -- --format 2024 --speed 2 --laps 12 --loop
```

The scripted race includes the following events, so every monitor and radio call gets exercised:

- tyre wear and a fuel deficit
- wing damage and a 5-second penalty
- a safety car
- rain arriving
- pit stops

### Record and replay

```sh
cargo run --release -p pitwall-server -- --record           # writes captures/capture-<format>-<time>.bin
cargo run --release -p pitwall-server -- --replay captures/x.bin --replay-speed 5
cargo run --release -p f1-synth -- --format 2022 --no-send --speed 0 --record captures/synth.bin
```

## Radio voice (optional)

Without a TTS server, radio calls show as text on the pit wall. To hear them, run one of these and set `[tts]` in `config.toml` (copy `config.example.toml`):

- **Piper:** `python -m piper.http_server -m en_GB-alan-medium --port 5000`, then use `engine = "piper"`.
- **Kokoro:** run Kokoro-FastAPI on port 8880, then use `engine = "kokoro"` and `url = "http://127.0.0.1:8880"`.

Audio goes through a radio filter and plays on the default output device. Pick another device with `[audio] device = "..."`; run `pitwall-server --list-audio-devices` to see the names. Common phrases are cached in `tts-cache/`.

## Friend on the pit wall (Tailscale)

1. Install Tailscale on both machines.
2. Your friend opens `http://<your-tailscale-ip>:8080`.
3. To limit who can send radio, approve callouts or change modes, set a shared token: `pitwall-server --token <secret>` or `[http] token`. The friend enters the token under ⚙ in the top bar.

## Layout

```text
crates/f1-udp          per-year UDP codec → version-neutral packets; capture files
crates/race-state      RaceState, timing-line gaps, metrics, wire snapshot types, SQLite
crates/comms           rule engine + arbitrator (P0/P1/P2, gating, cooldowns, copilot)
crates/voice-comms     radio DSP, resampling, TTS cache, cpal playback
crates/ai-sidecar      TTS HTTP clients (STT/LLM in M5)
crates/pitwall-server  the daemon (ingest, actors, axum WS/REST, static UI)
tools/f1-synth         synthetic race generator
ui/                    SvelteKit pit wall
```

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cd ui && npm run check
cd ui && npm run dev    # Vite dev server on :5173, proxies /api and /ws to :8080
```
