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
| M5 Voice input: push-to-talk, whisper, telemetry-first answers + LLM, engineer persona | ✅ |
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

## Radio voice and talking to your engineer

Everything runs locally. Each piece is optional, and the daemon degrades gracefully: with no voice server, calls show as text; with no LLM, common questions are still answered from telemetry.

Copy `config.example.toml` to `config.toml` first. The defaults below already match it.

### 1. The engineer's voice: Kokoro

```sh
docker run -d --name kokoro -p 8880:8880 ghcr.io/remsky/kokoro-fastapi-cpu:latest
```

The default voice is `bm_george`, a calm, low British male. Try `bm_fable` or `bm_lewis`, or blend them with `voice = "bm_george+bm_fable"` under `[tts]`. Urgent calls come slightly quicker and routine calls slightly calmer; set the overall rate with `speed`. The CPU image keeps your GPU free for the game.

### 2. Hearing you: whisper.cpp

whisper.cpp (CPU build) and the `ggml-base.en` model live in `sidecars/whisper/` (git-ignored). To install them on another machine:
1. Extract `whisper-bin-x64.zip` from the whisper.cpp GitHub releases into `sidecars/whisper/bin`.
2. Put `ggml-base.en.bin` from huggingface.co/ggerganov/whisper.cpp in `sidecars/whisper/models`.

The daemon starts it automatically when nothing is listening on `:8178` (`[stt] autostart`, log in `data/whisper-server.log`). To run it yourself:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\start-whisper.ps1
```

It transcribes a short radio clip in about a second on the CPU. For better accuracy, download `ggml-small.en.bin` and pass `-Model ggml-small.en.bin`. Any OpenAI-compatible transcription server also works (`[stt] engine = "openai"`).

### 3. Thinking: Ollama (only for open-ended questions)

```sh
ollama pull qwen2.5:3b
```

`qwen2.5:3b` answers in under a second once loaded, but it is fairly generic. For sharper answers set `[llm] model = "llama3.1:8b"`, if your VRAM can spare about 5 GB next to the game. Common questions never reach the LLM:
- gap ahead or behind, the leader
- tyres, including "will they make the end?", and temperatures
- fuel, battery, brakes, damage, weather, pace
- position, laps left, penalties, flags
- "should I box?"

### 4. Push-to-talk

1. In the game, go to **Controls**. Map a wheel button to **UDP Action 1** (or set `[voice] ptt_button`). Make sure UDP telemetry is on.
2. Hold the button while you talk and release to send, or tap it once, talk, and tap again to send. The engineer's line cuts out and you hear a chirp when the channel opens.
3. Without a wheel binding, use the **Hold to talk** button on the driver view (`/driver`) or in the AI Engineer monitor. That monitor also has an **Ask engineer** box for typed questions.

Run `pitwall-server --list-audio-devices` to see speaker and microphone names for `[audio] device` and `[voice] input_device`. Set `[comms] driver_name` so the engineer can use your first name now and then.

Check it is all up in the AI Engineer monitor: **MIC**, **SPEECH-TO-TEXT**, **LLM** and **VOICE** should all read ready.

#### Push-to-talk troubleshooting (AI Engineer monitor → Driver voice)

| Symptom | Fix |
| :--- | :--- |
| The level meter doesn't move when you speak | Pick another microphone in the dropdown; the choice is remembered. Virtual devices (Steam Streaming, Stereo Mix) are greyed out. |
| "This microphone sends pure silence" | Windows is blocking desktop apps: **Settings → Privacy & security → Microphone → Let desktop apps access your microphone**. |
| "Waiting for a press of UDP Action 1" never changes while you press the button | The game isn't sending it. Check **Settings → Telemetry → UDP Telemetry On** and that the button is bound to **UDP Action 1** under Controls (in the action list, not just the menu shortcut). |
| "Game sent UDP Action 3, but push-to-talk listens to UDP Action 1" | Re-bind the button, or set `[voice] ptt_button = 3`. |
| The engineer says "the speech server is down" | whisper.cpp isn't running and couldn't be auto-started; see step 2. |

The server log also prints every wheel button press and each push-to-talk clip (`push-to-talk released: … ms held, level …`).

## Friend on the pit wall (Tailscale)

1. Install Tailscale on both machines.
2. Your friend opens `http://<your-tailscale-ip>:8080`.
3. To limit who can send radio, approve callouts or change modes, set a shared token: `pitwall-server --token <secret>` or `[http] token`. The friend enters the token under ⚙ in the top bar.

## Layout

```text
crates/f1-udp          per-year UDP codec → version-neutral packets; capture files
crates/race-state      RaceState, timing-line gaps, metrics, wire snapshot types, SQLite
crates/comms           rule engine + persona, arbitrator (P0/P1/P2, gating, cooldowns, copilot), driver Q&A
crates/voice-comms     radio DSP, speech text normalisation, TTS cache, cpal playback + mic capture
crates/ai-sidecar      HTTP clients: TTS (Kokoro/Piper), STT (whisper), LLM (Ollama/OpenAI-compatible)
crates/pitwall-server  the daemon (ingest, actors, voice/PTT, axum WS/REST, static UI)
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
