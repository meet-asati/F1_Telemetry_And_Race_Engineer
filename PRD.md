# Product Requirements Document (PRD): F1 22 AI Race Engineer & Duo Pit Wall

- **Target Game & Spec:** EA Sports F1 22 — UDP Format `2022` (24-byte header, packet IDs `0–11`, up to 22 cars)
- **Execution Mode:** Live driving (`127.0.0.1:20777`, `20Hz` or `60Hz`) + offline `.bin` packet replay
- **Core Runtime:** Rust workspace (`tokio`, `axum`, `sqlx`, `serenity`, `songbird`, `cpal`, `plotters`)
- **Frontend Stack:** SvelteKit / Vite + TypeScript + HTML5 `<canvas>` over Tailscale
- **Local AI Sidecars:** HTTP services (`llama.cpp` / `Ollama`, `whisper.cpp`, `Piper` / `Kokoro`)

---

## 1. Problem Statement & Vision

Sim racing solo in F1 22 makes it hard to track tyre degradation curves, fuel delta, ERS deployment, weather shifts, and live gaps while pushing at the limit. Standard telemetry overlays display raw numbers rather than giving concise, situational race-engineer advice.

This project builds a **Rust-backed AI Race Engineer and Duo Pit-Wall system** where:
1. A high-performance Rust daemon ingests live F1 22 UDP packets, computes derived race strategy metrics, and drives a local LLM + TTS radio pipeline without starving game/GPU resources.
2. A friend can join as a **live human race engineer** via a browser pit-wall dashboard (connected over Tailscale) and a shared Discord voice channel—collaborating with, overriding, or approving AI callouts in real time.

---

## 2. Scope & Design Choices

| Area | Selected Decision | Rationale |
| :--- | :--- | :--- |
| **Game & UDP Spec** | F1 22 (`format = 2022`, IDs `0–11`) | Clean, fixed 22-car layout; supports `BUTN` custom UDP wheel triggers. |
| **AI Inference Runtime** | Separate local HTTP sidecars | Avoids heavy C++/CUDA build friction in Rust; isolates GPU VRAM & allows hot-swapping models. |
| **Voice Triggering** | F1 `BUTN` (`UDP 1`) PTT + Wake Word/VAD fallback | Reliable wheel-button PTT for explicit queries plus hands-free fallback without constant false triggers. |
| **Comms & Authority** | Tiered priority queue + straight-line gating + friend override & Copilot approval | Prevents chatter in braking zones, respects friend voice/radio priority, and supports human-in-the-loop approval. |
| **Networking & Hosting** | Embedded `axum` server + Tailscale | Encrypted peer-to-peer access (`100.x.y.z:8080`) with no router port-forwarding. |
| **Pit-Wall Frontend** | Svelte/Vite + TypeScript + `<canvas>` | Fast `15Hz` WebSocket updates and smooth 22-car track map & telemetry rendering. |
| **Persistence & Debrief** | Raw `.bin` recorder + SQLite (`sqlx`) + `plotters` PNG Discord post | Enables deterministic offline replay, historical stint/lap queries, and automated post-race summaries. |

---

## 3. Functional Requirements

### 3.1 F1 22 UDP Ingestion & Recorder (`f1-udp-2022`)
- Bind a UDP socket on `127.0.0.1:20777` (configurable) and validate `PacketHeader.m_packetFormat == 2022` (24-byte little-endian packed header).
- Parse all 12 F1 22 packet types (`0` Motion through `11` Session History), handling variable/cycled packet frequencies and player car index (`m_playerCarIndex`).
- Extract `BUTN` events (`m_buttonStatus` in Event packet ID `3`) to detect bound wheel triggers (`UDP 1`).
- **Packet Recorder & Replay:** Optionally record raw datagrams with microsecond timestamps and packet lengths to a `.bin` file, and replay captures at `1x`, `5x`, or `10x` speed for deterministic testing.

### 3.2 Derived State & SQLite Persistence (`race-state`)
- Maintain live in-memory `RaceState` covering session info, weather/forecast, 22-car positions/gaps/lap times, tyre compounds/age/wear/temps, brake temps, fuel remaining, ERS energy/mode, and damage.
- Compute derived metrics:
  - Fuel margin / estimated laps remaining vs. laps left in race.
  - Tyre wear rate (% per lap) and thermal trend per corner.
  - Rolling lap/sector pace deltas and gap trends (closing/opening).
- Persist `sessions`, `laps`, and `radio_events` in SQLite via `sqlx` (including handling session resets and `FLBK` flashback events).

### 3.3 Voice Input, Local AI Sidecars & Radio Audio (`voice-comms`, `ai-sidecar`)
- **Push-to-Talk (`UDP 1`):** Capture 16kHz mono PCM via `cpal` while `UDP 1` is active; send audio to `whisper.cpp` HTTP endpoint for transcription.
- **Wake Word / VAD Fallback:** Monitor mic energy/VAD + wake phrase when `UDP 1` is inactive and the user is not actively speaking on Discord.
- **LLM Engineer:** Send compact JSON `RaceState` + user query or trigger event to `llama.cpp` / `Ollama` HTTP API with a concise, factual race-engineer system prompt (grounded strictly in provided telemetry).
- **TTS & Radio DSP:** Synthesize speech via `Piper` or `Kokoro` HTTP endpoint, apply a radio bandpass filter (`300Hz–3400Hz`) + light static/compression, and resample to `48kHz` stereo for Discord voice playback.

### 3.4 Comms Arbitrator & Duo Mode (`3A + 3B`)
- **Priority Tiers:**
  - **P0 (Immediate):** `SC`, `VSC`, `Red Flag`, `Penalty` (`PENA`), and friend manual radio messages.
  - **P1 (Driver Query):** PTT or wake-word questions from the driver.
  - **P2 (Proactive Callouts):** Tyre wear/temp alerts, fuel warnings, rain/pit suggestions.
- **Straight-Line Gating:** Hold P2 callouts until `throttle > 0.85` and `|steer| < 0.15` (with a max queue age so stale tips expire).
- **Friend Override & Copilot Approval:**
  - Suppress P2 AI speech for a configurable cooldown (default `10s`) when the friend sends a radio command or speaks on Discord.
  - Support toggling **Auto-Speak** vs. **Copilot Approval** mode, where P2 AI suggestions surface as actionable cards on the friend's dashboard (`Approve`, `Edit & Send`, `Dismiss`).

### 3.5 Web Pit-Wall Dashboard & Discord Bot (`pitwall-server`, `ui`)
- **Web Dashboard (`axum` + Svelte/Vite):**
  - Stream `15Hz` JSON state snapshots over WebSockets.
  - Render a 22-car `<canvas>` track map, timing tower & gaps, 4-corner tyre/brake/ERS/fuel telemetry, event feed, Copilot approval queue, and quick/custom text-to-radio controls.
  - Support optional minimal driver overlay mode.
- **Discord Integration (`serenity` + `songbird`):**
  - Join the shared voice channel and stream radio-filtered TTS audio so both driver and friend hear radio calls.
  - Provide slash commands (`/status`, `/tyres`, `/gaps`, `/radio <msg>`, `/mode <auto|copilot>`) and post a `plotters`-generated race debrief PNG + stint summary on `CHQF` / `FinalClassification`.

---

## 4. Non-Functional Requirements

- **Performance & Frame Budget:** Rust UDP parsing, state updates, and `15Hz` WebSocket fan-out should use minimal CPU/memory with zero GC pauses and avoid competing with F1 22's GPU frame pacing.
- **Latency Targets:**
  - UDP packet-to-state update: `< 5ms`.
  - Push-to-talk end-to-end (STT + LLM + TTS + radio play): target `< 2.5s` on local hardware.
- **Reliability & Fault Isolation:** Sidecar HTTP timeouts or restarts must never crash the Rust UDP loop or drop WebSocket/Discord connections.
- **Security:** Dashboard intended for trusted Tailscale peers; support an optional shared session token for radio/command endpoints.

---

## 5. Workspace & Milestone Plan

```text
f1-pitwall/
├── Cargo.toml
├── crates/
│   ├── f1-udp-2022/
│   ├── race-state/
│   ├── voice-comms/
│   ├── ai-sidecar/
│   └── pitwall-server/
└── ui/
