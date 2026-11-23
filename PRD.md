# Product Requirements Document (PRD): F1 AI Race Engineer & Duo Pit Wall

- **Target Games & UDP Specs:** EA Sports F1 22, F1 23, F1 24, F1 25. UDP formats `2022` / `2023` / `2024` / `2025` are detected from `m_packetFormat`. There are up to 22 cars.
- **Execution Mode:**
  - Live telemetry: bind `0.0.0.0:20777` (configurable). The game's send rate is set in-game to `10–60Hz`, and `20Hz` or higher is recommended.
  - Offline replay of recorded `.bin` captures.
  - A synthetic race generator for development.
- **Core Runtime:** Rust workspace (`tokio`, `axum`, `binrw`, `sqlx`, `cpal`, `rubato`, `reqwest`). Added later: `serenity`, `songbird`, `plotters`.
- **Form Factor:** A headless Rust daemon (`pitwall-server`) runs on the gaming PC and serves a **web app** (SvelteKit / Vite + TypeScript + HTML5 `<canvas>`). The web app is reached locally or over Tailscale. A Tauri tray shell is an optional later add-on.
- **Local AI Sidecars:** HTTP services:
  - `Ollama` / `llama.cpp` (LLM)
  - `whisper.cpp` server (STT)
  - `Piper` / `Kokoro` (TTS)

  Each can run locally or on another Tailscale machine.

---

## 1. Problem Statement & Vision

Sim racing solo in F1 games makes it hard to track several things while pushing at the limit: tyre degradation curves, fuel delta, ERS deployment, weather shifts and live gaps. Standard telemetry overlays display raw numbers. They don't give concise, situational race-engineer advice.

This project builds a **Rust-backed AI Race Engineer and Duo Pit-Wall system**:

1. **A high-performance Rust daemon.** It ingests live F1 UDP packets from any supported game year, computes derived race-strategy metrics, and drives a radio pipeline. The pipeline is:
   - a deterministic rule engine;
   - a small local LLM for driver questions;
   - local TTS.

   None of this should starve the game of CPU/GPU resources.
2. **A friend as a live human race engineer.** They join through a browser pit-wall dashboard over Tailscale, and optionally a shared Discord voice channel. From there they collaborate with, override or approve AI callouts in real time.

---

## 2. Scope & Design Choices

| Area | Selected Decision | Rationale |
| :--- | :--- | :--- |
| **Games & UDP Specs** | F1 22 / 23 / 24 / 25. Per-year decoders map to one version-neutral `TelemetryFrame` model. | The player can switch game years without touching state, AI or UI code. Newer games can also emit older formats through the in-game "UDP Format" setting. |
| **Deployment & Form Factor** | Headless Rust daemon + web app served by `axum`. Optional Tauri tray shell later. | The remote friend needs zero-install browser access. Pop-out monitors are just browser windows, and all UI code is reused if a desktop shell is added. |
| **AI Inference Runtime** | Separate local HTTP sidecars, with URLs configurable per sidecar. | Avoids heavy C++/CUDA build friction in Rust. It also isolates GPU VRAM, allows hot-swapping models, and lets the sidecars move to another machine. |
| **AI Strategy (8 GB VRAM budget)** | A **deterministic rule engine** produces all proactive (P0/P2) callouts. A **small quantized LLM** (~3–4B, Q4) answers free-form driver questions only (P1). | Proactive callouts have no hallucination and low latency, and the GPU stays free for the game. The LLM is used where free-form language actually adds value. |
| **Voice Triggering** | F1 `BUTN` (`UDP Action 1–12`, bindable) PTT, with a wake word/VAD fallback. | Reliable wheel-button PTT for explicit queries, plus a hands-free fallback that doesn't fire constantly by mistake. |
| **Comms & Authority** | Tiered priority queue + straight-line gating + per-alert cooldowns + friend override & Copilot approval. | Prevents chatter in braking zones and repeated nagging. Respects friend voice/radio priority and supports human-in-the-loop approval. |
| **Radio Audio Output** | **Local playback first** (`cpal`, selectable device). Discord voice streaming is a later milestone. | Works solo with no bot setup. Discord adds shared listening once the core is solid. |
| **Networking & Hosting** | Embedded `axum` server + Tailscale. | Encrypted peer-to-peer access (`100.x.y.z:8080`) with no router port-forwarding. |
| **Pit-Wall Frontend** | Svelte 5 / Vite + TypeScript + `<canvas>`. | Fast `15Hz` WebSocket updates and smooth rendering of the 22-car track map and telemetry. |
| **Persistence & Debrief** | Raw `.bin` recorder + SQLite (`sqlx`) + `plotters` PNG (later). | Enables deterministic offline replay, historical stint/lap queries and automated post-race summaries. |

---

## 3. Functional Requirements

### 3.1 Multi-Format F1 UDP Ingestion & Recorder (`f1-udp`)

#### Socket and header

- Bind a UDP socket on `0.0.0.0:20777` (configurable host and port, so a console or second PC can send to it). Optionally restrict which source IPs are accepted.
- Read `m_packetFormat` (the first `u16`, little-endian) and hand off to the matching decoder (`2022`, `2023`, `2024`, `2025`). Unknown formats are counted and logged, not treated as fatal.
- The header differs between years:
  - **F1 22 header: 24 bytes.** `packetFormat`, `gameMajorVersion`, `gameMinorVersion`, `packetVersion`, `packetId`, `sessionUID`, `sessionTime`, `frameIdentifier`, `playerCarIndex`, `secondaryPlayerCarIndex`.
  - **F1 23+ header: 29 bytes.** Adds `m_gameYear` and `m_overallFrameIdentifier`.

#### Packet types

- Packet IDs by year:

  | ID | Packet | Games |
  | :--- | :--- | :--- |
  | `0` | Motion | All years |
  | `1` | Session | All years |
  | `2` | Lap Data | All years |
  | `3` | Event | All years |
  | `4` | Participants | All years |
  | `5` | Car Setups | All years |
  | `6` | Car Telemetry | All years |
  | `7` | Car Status | All years |
  | `8` | Final Classification | All years |
  | `9` | Lobby Info | All years |
  | `10` | Car Damage | All years |
  | `11` | Session History | All years |
  | `12` | Tyre Sets | F1 23+ |
  | `13` | Motion Ex | F1 23+ (player-only extended motion) |
  | `14` | Time Trial | F1 24+ |
  | `15` | Lap Positions | F1 25 |

  Unknown or unused packet IDs are skipped.
- Every packet struct is packed little-endian. The decoders use `binrw` (read + write), and the expected size of each packet is asserted per format in unit tests against the official EA spec.
- Handle the player car index (`m_playerCarIndex`) and the fact that packets arrive at different, cycling frequencies.

#### Normalization into `TelemetryFrame`

- **Wheel arrays.** F1 sends wheel arrays as `RL, RR, FL, FR`. The decoders **must** reorder them to `FL, FR, RL, RR` before anything downstream sees them.
- **Restricted telemetry.** In online sessions, a car whose `m_yourTelemetry` = restricted reports zeros for Car Status, Car Damage and Car Setups. Normalize those fields to `None`/"restricted". Do not pass the zeros through as real values.
- **Native gap deltas.** F1 23+ Lap Data includes native gap deltas (`deltaToCarInFront` / `deltaToRaceLeader`). Expose them when present.

#### PTT button events

- Extract `BUTN` events (`m_buttonStatus` bit flags in the Event packet) for the bindable `UDP Action 1–12` buttons.

#### Packet recorder and replay

- Optionally record raw datagrams to a `.bin` file:
  - **File header:** magic `F1PW`, file version, game format.
  - **Records:** `(u64 t_us, u16 len, payload)`.
- Replay captures at `1x`, `5x` or `10x` speed for deterministic testing.

### 3.2 Derived State & SQLite Persistence (`race-state`)

#### Live state

- Maintain a live in-memory `RaceState`, owned by a single-writer state actor. It covers:
  - session info, and weather plus the structured forecast (`m_weatherForecastSamples[]`, `m_forecastAccuracy`);
  - marshal zones (`m_marshalZones[]`);
  - Safety Car status;
  - for all 22 cars: positions, gaps, lap/sector times, and penalties and warnings;
  - tyre compound (actual + visual), age, wear, surface/inner temps and tyre damage (plus blisters on F1 25, the first format to send them);
  - brake temps and brake damage;
  - fuel remaining and the MFD fuel delta (`m_fuelRemainingLaps`);
  - ERS energy, deploy mode, and harvest/deploy this lap;
  - full damage and component wear: wings, floor, diffuser, sidepods, DRS/ERS faults, ICE / MGU-H / MGU-K / ES / CE / TC (turbocharger) / gearbox, and engine blown/seized.

#### Derived metrics

- **Gaps.** F1 22 provides no gap fields, so gaps are computed from **distance-based timing lines**: a timestamp for each car every 50 m of lap distance, with the gap taken at the latest shared line. When the game provides native deltas (F1 23+), use those and keep the computed gap as a cross-check.
- **Fuel.** Fuel margin, burn per lap vs. the burn required to finish, and estimated lift-and-coast savings.
- **Tyres.** Wear rate (% per lap) per corner, a linear degradation slope, and a projected pit lap. This is combined with the game's own pit window (`m_pitStopWindowIdealLap`, `m_pitStopWindowLatestLap`, `m_pitStopRejoinPosition`).
- **Thermal.** Thermal trend per corner (surface vs. inner vs. brake).
- **Pace.** Rolling lap/sector pace deltas and gap trends (closing or opening).

#### SQLite persistence (`sqlx`, embedded migrations)

- **Tables:**
  - `sessions`: session UID, game year, track, session type, started at.
  - `laps`: session, car index, lap, lap time, S1/S2/S3, compound, tyre age, wear ×4, fuel at lap end, ERS harvested/deployed, valid flag.
  - `stints`: session, car, compound, start lap, end lap.
  - `radio_events`: time, priority, source (`rule` / `llm` / `friend` / `driver`), text, context JSON, outcome (`spoken` / `dismissed` / `expired`).
  - `track_outlines`: track ID → simplified polyline built from a clean player lap.
- **Session resets.** A new `m_sessionUID` starts a new session.
- **Flashbacks.** A `FLBK` event, or the frame/lap counters going backwards, deletes all rows after the flashback point.

### 3.3 Voice Input, Local AI Sidecars & Radio Audio (`voice-comms`, `ai-sidecar`)

#### Push-to-talk (bindable `UDP Action N`)

- The microphone stream stays open (with a `300ms` pre-roll, so the first word is never clipped). While the button is held, `cpal` captures mono PCM. On release it is resampled to 16kHz and sent to the `whisper.cpp` server (or any OpenAI-compatible `/v1/audio/transcriptions` endpoint) with a racing-vocabulary prompt.
- **Half-duplex radio.** Keying up stops the engineer mid-sentence and plays a short chirp. Only P0 calls play while the driver is transmitting.
- **Stuck-PTT guard.** `BUTN` events are sent on state change over lossy UDP, so a lost release packet could leave PTT stuck. Enforce a maximum capture length (default `15s`).
- **Optional toggle mode.** Press to start, press to stop.
- **Other inputs.** A hold-to-talk button on the driver view and the Comms monitor uses the same path (`ptt.set`). Typed questions (`driver.ask`) skip the microphone.
- Clips under `350ms` or near-silent are dropped, and whisper's silence hallucinations ("Thank you.") are filtered out.

#### Wake word / VAD fallback *(deferred)*

- Monitor mic energy/VAD plus a wake phrase, but only while PTT is inactive and the user isn't actively speaking on Discord. Not built in M5; wheel PTT covers the need.

#### Rule engine (proactive callouts)

- Deterministic rules evaluate `RaceState`. Each rule has a key, a priority, thresholds, a cooldown, and a set of phrase templates with varied wording. Examples:
  - tyre wear or temperature thresholds;
  - a wear slope projecting a cliff before the pit window;
  - fuel delta below `-0.3` laps;
  - Safety Car / VSC / red flag deployed or ending;
  - forecast rain % rising;
  - penalty issued;
  - defend (car behind within `1.0s`) and push (gap ahead closing);
  - wing or floor damage.
- No LLM is involved, so callouts can't hallucinate.

#### Engineer persona

- One consistent voice across callouts, answers and the LLM: a calm, precise, warm British race engineer. Information first, then at most one clear instruction. Rivals are named by surname. Praise is given when it is earned (personal best, fastest lap, a pass, a good stop, the finish). Words are kept to what the moment allows.
- Proactive "race moments" on top of the warnings: personal best, fastest lap, positions gained or lost, good pit stop and fitted compound, five laps to go, last lap, and a result-aware chequered-flag line. The driver's first name (`[comms] driver_name`) is used sparingly.

#### Driver questions: telemetry first, LLM second

- A deterministic answerer handles the common radio questions straight from `RaceState`, instantly and always correctly: radio check, gap ahead/behind/leader with a pace comparison, tyres (including "will they make the end?"), temperatures, brakes, fuel, ERS, position, laps left, damage, weather, pace, penalties, flags, strategy, and "should I box?" (a yes/no decision).
- Open-ended questions ("what if…", "why…", "what should…") and anything unrecognised go to the LLM. The engineer first says "Copy, stand by."
- The LLM (`Ollama` `/api/chat`, or any OpenAI-compatible `/v1/chat/completions`) gets a compact text race brief and the persona system prompt. The prompt allows at most two radio sentences, using only facts from the brief, with no lists, markdown or "as an AI". Replies are trimmed to two sentences.
- **Default model:** `qwen2.5:3b` (Q4, about 2 GB). Larger models such as `llama3.1:8b` also work if VRAM allows alongside the game. The model is warmed up at start and kept loaded (`keep_alive`). Health only reports ready when the configured model is actually pulled.

#### TTS and radio DSP

- Synthesize speech through a single TTS interface, which supports:
  - `Kokoro` (default; OpenAI-compatible `/v1/audio/speech`, voice `bm_george`, blends such as `bm_george+bm_fable`);
  - `Piper` (HTTP server).
- **Delivery.** P0 calls are synthesized about 8% quicker and routine P2 calls about 3% calmer. Text is normalised for speech first: lap times as engineers say them ("one oh-seven point four"), signs ("minus 0.3"), units ("percent", "degrees", "seconds") and abbreviations ("D-R-S", "front-left").
- Templated phrases are synthesized once and **cached on disk by text hash**.
- Apply a radio bandpass filter (`300Hz–3400Hz`), soft compression and light static. Then resample (`rubato`) to the output device rate, and later to `48kHz` stereo for Discord.
- **Graceful degradation.** If no TTS sidecar is reachable, callouts still appear as text in the pit-wall radio log and the daemon keeps running.

#### Local playback

- Play radio audio through a selectable local output device (`cpal`, on a dedicated audio thread with a ring buffer).

### 3.4 Comms Arbitrator & Duo Mode

#### Priority tiers

- **P0 (Immediate):** Safety Car, VSC, red flag, penalty (`PENA`), and the friend's manual radio messages. Spoken immediately; **preempts** a P2 line that is already playing.
- **P1 (Driver Query):** answers to PTT or wake-word questions. Spoken as soon as they're ready, with no straight-line gating, because the driver asked.
- **P2 (Proactive Callouts):** tyre, fuel, rain, pit and gap suggestions from the rule engine.

#### Straight-line gating

- Hold P2 callouts until `throttle > 0.85` and `|steer| < 0.15`.
- A P2 callout expires after a maximum queue age (default `20s`).

#### De-duplication and cooldowns

- Each alert key has its own cooldown, so the same issue isn't announced every lap. A cooldown re-arms only when severity escalates (`watch` → `critical`).

#### Speech-free windows

- No P2 speech during the race start / formation-lap launch, or while the car is in the pit lane (`m_pitStatus`).

#### Friend override and Copilot approval

- When the friend sends a radio command or speaks on Discord, suppress P2 AI speech for a configurable cooldown (default `10s`).
- Support toggling **Auto-Speak** vs. **Copilot Approval** mode. In Copilot Approval mode, P2 suggestions appear as actionable cards on the friend's dashboard (`Approve`, `Edit & Send`, `Dismiss`) and expire after the same maximum queue age.

### 3.5 Web Pit-Wall Dashboard (`pitwall-server`, `ui`)

#### Server

- **Snapshots.** Stream `15Hz` JSON state snapshots over WebSockets. Each snapshot is serialized once per tick and the bytes are shared with all clients.
- **Events and commands.** Push typed event messages, and accept typed client commands (see `UI.md` §5).
- **REST.**
  - `GET /api/health`
  - `GET /api/sessions`
  - `GET /api/sessions/:id/laps`
  - `GET /api/track-outline/:trackId`
  - `POST /api/radio`
- **Static UI.** Serve the built UI (`ui/build`) as static files.

#### Frontend

- Renders:
  - the 22-car `<canvas>` track map;
  - the timing tower and gaps;
  - 4-corner tyre/brake telemetry, plus ERS/fuel and damage;
  - weather;
  - the event feed;
  - the Copilot approval queue;
  - quick and custom text-to-radio controls.
- A minimal **driver view** (`/driver`) for a phone, tablet or second screen.

### 3.6 Discord Integration (later milestone: `serenity` + `songbird`)

- Join the shared voice channel and stream radio-filtered TTS audio, so both the driver and the friend hear radio calls.
- Detect the friend speaking (voice receive / speaking events) to trigger the override cooldown.
- **Before building:** verify that the chosen `songbird` version supports Discord's current voice end-to-end encryption (DAVE).
- Provide slash commands: `/status`, `/tyres`, `/gaps`, `/radio <msg>`, `/mode <auto|copilot>`.
- On `CHQF` / `FinalClassification`, post a `plotters`-generated race debrief PNG and a stint summary.

---

## 4. Non-Functional Requirements

- **Performance & Frame Budget.** Rust UDP parsing, state updates and the `15Hz` WebSocket fan-out should use minimal CPU and memory, with no GC pauses. They should not compete with the game's GPU frame pacing.
- **Latency Targets:**
  - UDP packet to state update: `< 5ms`.
  - Rule-based callout, from gating release to audio start: `< 1s` (cached TTS: `< 300ms`).
  - Push-to-talk end-to-end (STT + LLM + TTS + radio play): target `< 2.5s` on local hardware.
- **VRAM Budget.** Default sidecar choices must leave the game enough VRAM on an 8 GB GPU:
  - a small Q4 LLM;
  - Whisper `base.en`/`small.en` on the CPU;
  - a CPU-friendly TTS.
- **Reliability & Fault Isolation.**
  - Sidecar HTTP calls have timeouts and run on their own tasks. A sidecar timing out or restarting must never crash the UDP loop or drop WebSocket/Discord connections.
  - The audio thread is isolated from the async runtime.
- **Security.**
  - The dashboard is intended for trusted Tailscale peers. It can be bound to the Tailscale IP only.
  - An optional shared session token is required for **every mutating** WebSocket command and REST `POST`.

---

## 5. Developer Experience & Testing

- **`f1-synth`.** Generates a synthetic 22-car race and sends it as real UDP datagrams in any supported format. Scenarios include tyre degradation, a Safety Car period and rain onset. It lets the UI and the AI pipeline be developed without the game running.
- **Golden tests.** For every format and packet type: encode → decode round-trips plus packet-size assertions.
- **Unit tests.**
  - wheel-order normalization;
  - gap timing lines;
  - wear-slope and fuel projections;
  - flashback rollback;
  - arbitrator gating, cooldowns and preemption;
  - Copilot approve/dismiss.
- **Replay determinism.** Replaying a recorded `.bin` must produce identical lap rows in SQLite.

---

## 6. Workspace & Milestone Plan

```text
F1 Race Engineer/
├── Cargo.toml              # Rust workspace
├── config.example.toml
├── crates/
│   ├── f1-udp/             # per-year decoders (binrw) → TelemetryFrame, recorder/replay
│   ├── race-state/         # RaceState actor, derived metrics, SQLite
│   ├── comms/              # rule engine, phrase templates, arbitrator, copilot queue
│   ├── voice-comms/        # cpal I/O, radio DSP, resampling, TTS cache
│   ├── ai-sidecar/         # HTTP clients: TTS (Piper/Kokoro), STT (whisper.cpp), LLM (Ollama)
│   └── pitwall-server/     # binary: config, wiring, axum WS/REST, static UI
├── tools/
│   └── f1-synth/           # synthetic race generator
└── ui/                     # SvelteKit pit wall
```

| Milestone | Scope |
| :--- | :--- |
| **M1** | `f1-udp`: multi-format decode (2022–2025), recorder/replay, `f1-synth`. |
| **M2** | `race-state`: live state, derived metrics (gaps, wear slope, fuel, pit window), SQLite persistence. |
| **M3** | `pitwall-server` + `ui`: WebSocket/REST, the 7-monitor pit wall, inspectors, presets, pop-outs, driver view. |
| **M4** | `comms` + `voice-comms` + TTS: rule engine, arbitrator, local radio playback, Copilot approval flow. *(End of first build.)* |
| **M5** | Voice input: `BUTN` PTT (hold/toggle), `cpal` capture, whisper STT, telemetry-first driver Q&A + LLM fallback, engineer persona, Kokoro voice. *(Wake word/VAD deferred.)* |
| **M6** | Discord: `songbird` voice streaming, friend-speaking override, slash commands. |
| **M7** | Debrief: `plotters` PNG + stint summary (Discord post + UI). |
| **M8** *(optional)* | Tauri tray shell: start/stop, audio-device picker, autostart. Reuses the same UI and daemon. |
