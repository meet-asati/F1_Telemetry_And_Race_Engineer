# UI/UX Specification (`UI.md`): F1 Multi-Monitor Pit Wall

- **Frontend Stack:** SvelteKit (Svelte 5 runes, `adapter-static`) / Vite + TypeScript + HTML5 `<canvas>`. Charts are lightweight inline SVG primitives, with no heavy chart library.
- **Backend Transport:**
  - A WebSocket stream (`15Hz` state snapshots + typed event messages + typed client commands).
  - REST endpoints served by the Rust `axum` daemon over localhost / Tailscale.
- **Form Factor:** A web app served by the headless `pitwall-server` daemon:
  - `http://localhost:8080` locally;
  - `http://100.x.y.z:8080` for the remote friend.

  An optional Tauri tray shell can wrap the same UI later.
- **Primary Audience:** The human race engineer (the friend on the pit wall). A minimal driver view (`/driver`) serves the driver on a phone, tablet or second screen.
- **Supported Games:** F1 22 / 23 / 24 / 25. The UI is version-agnostic and only shows `gameYear` in the top bar.

---

## 1. Design Philosophy & Visual Language

The UI models an **F1 pit-wall monitor stand**: a structured, high-contrast command surface. Your friend can scan the critical race domains at a glance, then click **any monitor** (or metric or car) to open a deep-dive inspection view.

- **Physical Pit-Wall Look.** The page is the wall itself, and every domain is a real monitor mounted on it.
  - **Wall:** a carbon-fibre background with gunmetal uprights, a rail above each row of monitors and a desk ledge with a team-red accent line. The top bar is the overhead header panel.
  - **Monitors:** a gunmetal bezel, a mounting arm up to the rail, and a chin with the channel number (`PW · 03`) and a power LED that turns amber/red with the alert state.
  - **Screens:** near-black glass (`#02070A`) with a faint telemetry grid, light scanlines and a glass reflection. Ink is phosphor green (`#4DFA9A`) for data, cyan (`#22D3EE`) for headings and accents, amber for watch, red for critical.
  - **Telemetry styling:** wireframe top-down car diagrams, glowing traces and dense monospace columns, in the style of real team pit-wall screens.
  - **No emoji.** Icons are inline SVG line icons in the screen ink colour.
- **Tabular Precision.** Use a monospace font or `font-variant-numeric: tabular-nums` for lap times, gaps, temperatures, wear percentages and ERS/fuel deltas, so numbers never jitter as values tick at `15Hz`.
- **Semantic Alert States (`Nominal` | `Watch` | `Critical`).** Each state is always conveyed by **icon + text + color**, never color alone, so it works for color-blind users.
  - **Nominal:** Calm neutral/teal/slate indicators. No visual noise.
  - **Watch:** Amber border highlight + `▲ WATCH` badge + a short reason. Examples: tyre wear approaching the window, a slight fuel deficit, rain probability rising.
  - **Critical:** Red, high-contrast border + `■ CRITICAL` badge + a subtle pulse. The pulse is disabled under `prefers-reduced-motion`. Examples: high tyre wear or a temperature spike, wing/floor damage, Safety Car / VSC / penalty.
- **Unavailable vs. Restricted Data.**
  - Missing values render as `—`.
  - Rival data hidden by the game's "restricted telemetry" setting renders a 🔒 `Restricted` badge, never fake zeros.
- **Zero-Distraction Rule.** Monitor tiles must be scannable in `< 1.5s`. Put dense historical charts, raw packet fields and editing controls in the **Click-to-Inspect** view.

---

## 2. Workspace Layout & Navigation

### 2.1 Top Pit-Wall Bar

- **Session & Connection Strip.**
  - Session type (Practice / Quali / Race / Time Trial), current lap (`Lap X / Y`), track, weather icon and temperatures, and `gameYear`.
  - A **data source pill:** `Live` / `Replay` / `Synthetic` / `Waiting for game`.
  - A WebSocket latency/connection pill.
- **Stale-Data State.** If no snapshot arrives for `> 2s`, tiles grey out and show a `STALE` overlay. This makes a stall unmistakable rather than leaving frozen numbers on screen.
- **Layout Presets.** Switchable multi-monitor arrangements:
  - `Full Stand` (default 7-monitor grid)
  - `Stint & Tyres` (emphasizes Tyres, Power/Fuel and Timing)
  - `Quali & Pace` (emphasizes Track Map, Timing/Deltas and Comms)
  - `SC & Weather` (emphasizes Weather Radar, Timing and Damage)
- **Duo Mode & AI Controls.**
  - Toggle **Auto-Speak** vs. **Copilot Approval** mode.
  - Show an active **Friend Override** countdown badge (`overrideRemainingMs`) while the human engineer is speaking or sending radio messages.
  - A quick mute/unmute toggle for AI radio output.

### 2.2 Multi-Monitor Stand Grid

- **Monitor Tiles.** Each telemetry domain renders as a self-contained monitor card with:
  - a channel number (`01`–`07`), the monitor title and the `Nominal` / `Watch` / `Critical` badge;
  - compact, high-signal summary metrics designed for fast scanning.
- **Click anywhere on the tile** to open the **Monitor Inspector** for that domain.
- **Pop-Out Button** on each tile header opens that monitor in a standalone browser window (`/monitor/:id`), which is useful for multi-monitor setups.
- **Deep link:** `/?inspect=<monitor>` opens a monitor inspector directly (e.g. `/?inspect=tyres`).
  - Pop-outs share the selected car and the layout state with the main window via `BroadcastChannel`.
  - Each window keeps its own WebSocket.

### 2.3 Click-to-Inspect Monitor Popup

Clicking a monitor opens it as a large **in-window popup monitor** (same bezel and screen, about 94% of the window, over a blurred stand). `Esc`, the scrim or **Close** dismisses it; **Pop out** moves it to its own browser window. The popup contains:

1. **Full Telemetry Breakdown:** Every relevant field for that system, showing the current value, the previous lap and rolling averages.
2. **Historical & Comparative Curves:** Lap-by-lap progression, stint comparisons or per-corner values. History comes from `GET /api/sessions/:id/laps`.
3. **Live / Snapshot Mode Toggle:** Switch between live updates and inspecting a specific lap or event from the session.
4. **Quick "Send to Radio" Composer:** Pre-populated with context from the inspected metric or driver, with one-click send or custom text editing.

### 2.4 Driver View (`/driver`)

- A large-type, glanceable single screen for a phone, tablet or second monitor:
  - position, gap ahead/behind, last lap vs. best, fuel delta, ERS %, worst tyre wear;
  - the last radio exchange (the driver's question and the engineer's reply);
  - the Safety Car / VSC banner.
- Controls: a large **Hold to talk** button (touch-friendly) and mute. (An in-game overlay on top of a fullscreen game isn't practical, so it is out of scope.)

### 2.5 Rendering Budget

- Tiles re-render on each `15Hz` snapshot.
- Inspector charts are throttled to about `4Hz`.
- The track canvas runs on `requestAnimationFrame` and **interpolates car positions between snapshots** so dots move smoothly.

---

## 3. Monitor Specifications

### Monitor 1: Timing & Gap Tower

- **Tile Summary:** A compact standings list. Each row shows:
  - position and driver code/number;
  - gap to the car ahead or the leader;
  - current tyre compound + lap age;
  - pit count;
  - active penalty/warning tags.
- **Click-to-Inspect View:**
  - A full 22-car timing table: last lap, best lap, sector 1/2/3 times, pit status, tyre stint history, driver/result status.
  - A pace delta & gap trend chart against a selected rival or the car ahead.
  - Quick radio buttons (e.g., *"Push to catch car ahead"*, *"Manage gap / defend"*, *"Box next lap"*).

### Monitor 2: Live Track & Sector Map

- **Tile Summary:**
  - A `<canvas>` circuit outline built from Motion world coordinates (`worldPositionX`, `worldPositionZ`; Y is up). World +Z points up the map.
  - Live car dots, with the player car highlighted.
  - Per-zone flag markers from `marshalZones[]`.
  - Readouts for lap, last/best lap and delta, and the player's speed, gear and DRS.
  - A rolling 30-second trace of the player's throttle, brake and speed under the map.
- **Outline source:**
  - The server bins every running car's position by lap distance. Once 97% of the bins are filled (about one lap of green running with a full grid), it stores the outline per track (`GET /api/track-outline/:trackId`). The player does not need a clean lap.
  - Until then, the browser builds the same binned outline live from the snapshots, so the track draws in progressively within seconds.
- **Click-to-Inspect View:**
  - A zoomable, pannable track canvas with a color-mode selector (`Positions`, `Speed`, `Throttle/Brake`, `ERS Deploy`).
  - A telemetry readout for the selected car: speed, gear, throttle/brake bars, ERS deploy state, lap distance %.
  - A marshal-zone/flag list and an instant *"Warn traffic / flag in sector"* radio shortcut.

### Monitor 3: Tyre & Thermal Stand

- **Tile Summary:**
  - A 4-corner layout (`FL`, `FR`, `RL`, `RR`) showing tyre wear `%` and surface/inner temperatures.
  - A compound badge (`S`, `M`, `H`, `I`, `W`) and tyre age in laps.
- **Click-to-Inspect View:**
  - Per-corner wear rate (`%` lost per lap), plus the projected pit lap from the current degradation slope, shown next to the game's pit window (`ideal` / `latest` lap, rejoin position).
  - A thermal graph comparing surface vs. inner carcass temperatures and brake temperatures across recent laps.
  - Per-corner tyre damage `%` (plus blister `%` on F1 25).
  - Quick radio actions (e.g., *"Cool front tyres"*, *"Box for new set"*).

### Monitor 4: Power Unit, ERS & Fuel

- **Tile Summary:**
  - Fuel remaining vs. target (`+laps` / `-laps` delta from the MFD).
  - ERS battery percentage and the current deploy mode (`None`, `Medium`, `Hotlap`, `Overtake`).
  - Quick powertrain health status.
- **Click-to-Inspect View:**
  - Fuel burn per lap vs. the burn required to finish, plus estimated lift-and-coast savings.
  - ERS harvest (MGU-K + MGU-H) vs. deployment per lap, and a deployment profile against lap distance.
  - Component wear bars: `ICE`, `MGU-H`, `MGU-K`, `ES` (energy store), `CE` (control electronics), `TC` (**turbocharger**), gearbox. Also shows engine blown/seized and ERS fault flags.
  - Quick radio actions (e.g., *"Lift and coast for fuel"*, *"Deploy ERS / overtake mode"*).

### Monitor 5: Car Damage & Chassis/Brakes

- **Tile Summary:**
  - Front wing left/right damage `%`, rear wing damage `%`, floor and diffuser damage `%`.
  - A DRS fault flag and the peak brake temperature alert state.
- **Click-to-Inspect View:**
  - A visual chassis damage diagram highlighting the affected aero zones (wings, floor, diffuser, sidepods).
  - A front/rear brake temperature readout and brake damage `%`.
  - A current car setup summary (Car Setups: brake bias, diff, wings, suspension snapshot). Rival setups show as restricted.
  - Quick radio actions (e.g., *"Watch brake temps"*, *"Box for front wing repair"*).

### Monitor 6: Weather & Session Radar

- **Tile Summary:**
  - Current weather condition and air/track temperatures.
  - A rain forecast indicator: the next forecast sample with rain `%`, and the forecast accuracy.
  - Session laps/time remaining.
  - Safety Car / VSC status.
- **Click-to-Inspect View:**
  - A weather forecast timeline built from the structured forecast samples (rain probability, weather type and temperatures at each time offset).
  - A session & flag log (Safety Car, VSC, red flags, track limits/penalties).
  - Quick radio actions (e.g., *"Inters expected soon, stay out"*, *"Box for intermediates"*).

### Monitor 7: AI Engineer & Copilot Desk

- **Tile Summary:**
  - The active comms mode (`Auto-Speak` / `Copilot Approval`) and the mute state.
  - The pending AI suggestion count.
  - The driver's last radio message and the last engineer message (with on-air / thinking indicators).
  - Status: PTT (ready / live / no mic), speech-to-text, LLM and voice (TTS) health.
- **Click-to-Inspect View:**
  - **Copilot Approval Queue.** Pending P2 suggestions, each with:
    - a priority badge and an expiry countdown;
    - **Approve & Speak**, **Edit & Send** and **Dismiss** actions.
  - **Driver Voice.** Mic / speech-to-text / LLM / voice status chips, a **Hold to talk** button (keys the pit-wall PC's microphone), an **Ask engineer** box for typed driver questions, and quick questions ("Gap behind?", "Should I box this lap?").
  - **Manual Radio Console.** Quick preset buttons + free-text input. Messages go through the radio TTS pipeline to local playback, and to Discord voice when that is enabled.
  - **Radio & Prompt Audit Log.** A chronological feed of:
    - friend messages, rule callouts, LLM answers and driver voice queries;
    - each entry's outcome (`spoken` / `dismissed` / `expired`) and the telemetry context attached to it.

---

## 4. Cross-Monitor Interactions & Duo Mode Workflows

- **Click-a-Driver Cross-Highlight.** Selecting a driver in the Timing Tower or Track Map highlights that driver across related views, including pop-out windows via `BroadcastChannel`. The race engineer can then compare pace, tyres and gaps at a glance.
- **Context-Attached Radio.** Clicking an alert or metric inside any monitor's inspector lets the friend attach that exact reading (e.g., `FL wear 68%`, `Fuel -0.7 laps`) to a radio message before sending.
- **Copilot Approval vs. Override.**
  - In **Copilot Approval** mode, proactive (P2) AI recommendations wait for the friend's approval before being spoken, and expire if not acted on.
  - When the friend sends a radio message (or, once Discord is enabled, speaks on Discord), non-urgent P2 AI callouts are held or suppressed for the override cooldown window.
  - P0 calls (Safety Car / VSC / red flag / penalty) are never held.

---

## 5. WebSocket, REST & Frontend State Contract

The Svelte UI keeps a client-side `PitWallStore` (Svelte 5 runes). It is fed by typed messages from the Rust `axum` daemon over a reconnecting WebSocket at `/ws`.

**Conventions:**

- **Times** are integer **milliseconds** (`...Ms`).
- **Temperatures** are °C (`...C`).
- **Percentages** are `0–100` (`...Pct`).
- **Wheel arrays** are always ordered `[FL, FR, RL, RR]`. The server reorders them from the game's `RL, RR, FL, FR`.
- **`null`** means unavailable. Restricted rival data is flagged with `telemetryRestricted: true`, and its fields are `null`.

### 5.1 Messages, Snapshot and Commands

The canonical contract is `ui/src/lib/types.ts`, mirrored in Rust by `crates/race-state/src/snapshot.rs` (serde, camelCase). Notes:

- `lastPacketAgeMs` drives the stale/“Waiting for game” states (`null` = no packet yet).
- `session.phase` (`grid` | `start` | `racing` | `finished`) feeds the comms speech-free windows.
- `tyres.blistersPct` is only present for F1 25, which is the first format to send blister data.
- If the daemon has a shared token configured, every mutating command must include `token`. `select.car` never needs one.

```ts
// Wire contract with pitwall-server (UI.md §5). Mirrors crates/race-state/src/snapshot.rs.
// Times are integer ms, temps °C, percentages 0–100, wheel arrays [FL, FR, RL, RR].

export type AlertLevel = 'nominal' | 'watch' | 'critical';
export type Wheels<T> = [T, T, T, T];
export type DataSource = 'live' | 'replay' | 'synthetic' | 'none';
export type Priority = 'P0' | 'P1' | 'P2';
export type CommsMode = 'auto' | 'copilot';

export interface ForecastSample {
  timeOffsetMin: number;
  weather: string;
  rainPct: number;
  airTempC: number;
  trackTempC: number;
}

export interface SessionInfo {
  trackId: number;
  trackName: string;
  trackLengthM: number;
  sessionType: string;
  isRace: boolean;
  lap: number;
  totalLaps: number;
  timeLeftMs: number;
  weather: string;
  airTempC: number;
  trackTempC: number;
  forecastAccuracy: 'perfect' | 'approximate';
  weatherForecast: ForecastSample[];
  marshalZones: { startFraction: number; flag: 'none' | 'green' | 'blue' | 'yellow' | 'red' }[];
  safetyCarStatus: 'none' | 'full' | 'virtual' | 'formation';
  pitWindow: { idealLap: number; latestLap: number; rejoinPosition: number } | null;
  paused: boolean;
  phase: 'grid' | 'start' | 'racing' | 'finished' | '';
}

export interface Stint {
  compound: string;
  startLap: number;
  endLap: number | null;
}

export interface TimingEntry {
  carIndex: number;
  position: number;
  driverLabel: string;
  driverName: string;
  raceNumber: number;
  teamId: number;
  isPlayer: boolean;
  lap: number;
  gapToLeaderMs: number | null;
  gapToAheadMs: number | null;
  lastLapMs: number | null;
  bestLapMs: number | null;
  sectorsMs: [number | null, number | null, number | null];
  compound: string | null;
  tyreAgeLaps: number | null;
  pitStops: number;
  pitStatus: 'none' | 'pitting' | 'inPitArea';
  penaltiesMs: number;
  warnings: number;
  driverStatus: string;
  resultStatus: string;
  telemetryRestricted: boolean;
  stints: Stint[];
}

export interface TrackCar {
  carIndex: number;
  x: number;
  z: number;
  lapDistancePct: number;
  speedKph: number;
  gear: number;
  throttlePct: number;
  brakePct: number;
  steer: number;
  ersDeployMode: string | null;
  isPlayer: boolean;
  drsActive: boolean;
  inPit: boolean;
}

export interface Tyres {
  compound: string;
  actualCompound: string;
  ageLaps: number;
  wearPct: Wheels<number>;
  wearRatePerLap: Wheels<number | null>;
  damagePct: Wheels<number>;
  blistersPct: Wheels<number> | null;
  surfaceTempC: Wheels<number>;
  innerTempC: Wheels<number>;
  brakeTempC: Wheels<number>;
  pressurePsi: Wheels<number>;
  projectedPitLap: number | null;
  alertLevel: AlertLevel;
  alertReason: string | null;
}

export interface Powertrain {
  fuelRemainingLapsDelta: number;
  fuelRemainingKg: number;
  fuelBurnPerLapKg: number | null;
  fuelRequiredPerLapKg: number | null;
  ersStorePct: number;
  ersDeployMode: string;
  ersHarvestedThisLapKJ: number;
  ersDeployedThisLapKJ: number;
  wearPct: { ice: number; mguH: number; mguK: number; es: number; ce: number; tc: number; gearbox: number };
  engineBlown: boolean;
  engineSeized: boolean;
  ersFault: boolean;
  alertLevel: AlertLevel;
  alertReason: string | null;
}

export interface Setup {
  frontWing: number;
  rearWing: number;
  onThrottle: number;
  offThrottle: number;
  brakeBias: number;
  brakePressure: number;
  frontSuspension: number;
  rearSuspension: number;
  frontAntiRollBar: number;
  rearAntiRollBar: number;
  frontRideHeight: number;
  rearRideHeight: number;
}

export interface Damage {
  frontLeftWingPct: number;
  frontRightWingPct: number;
  rearWingPct: number;
  floorPct: number;
  diffuserPct: number;
  sidepodPct: number;
  brakeDamagePct: Wheels<number>;
  drsFault: boolean;
  setup: Setup | null;
  alertLevel: AlertLevel;
  alertReason: string | null;
}

export interface Suggestion {
  id: string;
  priority: Priority;
  category: string;
  message: string;
  createdAtMs: number;
  expiresAtMs: number;
  contextAttachments: string[];
}

export interface CommsState {
  mode: CommsMode;
  muted: boolean;
  friendOverrideActive: boolean;
  overrideRemainingMs: number;
  pttActive: boolean;
  ttsAvailable: boolean;
  /** Voice input: speech-to-text sidecar reachable. */
  sttAvailable: boolean;
  /** Voice input: LLM reachable with the configured model. */
  llmAvailable: boolean;
  /** A microphone is open for push-to-talk. */
  micAvailable: boolean;
  /** The engineer is transcribing / thinking about an answer. */
  thinking: boolean;
  /** Live microphone input level, 0..1. */
  micLevel: number;
  /** The microphone in use. */
  micDevice: string | null;
  /** The mic delivers pure digital silence (Windows privacy block / muted / dead). */
  micBlocked: boolean;
  /** Configured push-to-talk wheel button ("UDP Action N"). */
  pttButton: number;
  /** The last "UDP Action N" button the game reported. */
  lastUdpAction: number | null;
  speaking: boolean;
  lastSpokenLine: string | null;
  /** The driver's last radio message (transcript or typed question). */
  lastDriverLine: string | null;
  pendingSuggestions: Suggestion[];
}

export interface TelemetrySnapshot {
  schemaVersion: number;
  timestampMs: number;
  source: DataSource;
  lastPacketAgeMs: number | null;
  gameYear: number | null;
  sessionUid: string | null;
  session: SessionInfo;
  timing: {
    playerIndex: number;
    position: number;
    gapAheadMs: number | null;
    gapBehindMs: number | null;
    lastLapMs: number | null;
    bestLapMs: number | null;
    entries: TimingEntry[];
  };
  track: { outlineAvailable: boolean; cars: TrackCar[] };
  tyres: Tyres;
  powertrain: Powertrain;
  damage: Damage;
  comms: CommsState;
}

export type RadioSource = 'rule' | 'llm' | 'friend' | 'driver';
export type RadioOutcome = 'spoken' | 'queued' | 'dismissed' | 'expired' | 'textOnly' | 'suppressed';
export type FlagStatus = 'sc' | 'vsc' | 'scEnding' | 'vscEnding' | 'red' | 'green' | 'chequered';

export type RadioEvent = {
  kind: 'radio';
  atMs: number;
  source: RadioSource;
  priority: Priority;
  text: string;
  outcome: RadioOutcome;
  attachments: string[];
};

export type PitWallEvent =
  | RadioEvent
  | { kind: 'flag'; atMs: number; status: FlagStatus }
  | { kind: 'penalty'; atMs: number; carIndex: number; driverLabel: string; description: string; timeSec: number | null; lap: number }
  | { kind: 'suggestion.added'; suggestionId: string }
  | { kind: 'suggestion.resolved'; suggestionId: string; outcome: 'approved' | 'edited' | 'dismissed' | 'expired' }
  | { kind: 'session.reset'; sessionUid: string }
  | { kind: 'flashback'; atMs: number; toLap: number }
  | { kind: 'lap.completed'; carIndex: number; lap: number; lapMs: number };

export type ServerMessage =
  | { type: 'hello'; schemaVersion: number; serverVersion: string; authRequired: boolean }
  | { type: 'snapshot'; seq: number; data: TelemetrySnapshot }
  | { type: 'event'; event: PitWallEvent }
  | { type: 'error'; message: string };

export type ClientCommand =
  | { type: 'radio.send'; text: string; attachments?: string[]; token?: string }
  | { type: 'suggestion.approve'; id: string; token?: string }
  | { type: 'suggestion.edit'; id: string; text: string; token?: string }
  | { type: 'suggestion.dismiss'; id: string; token?: string }
  | { type: 'comms.setMode'; mode: CommsMode; token?: string }
  | { type: 'comms.mute'; muted: boolean; token?: string }
  | { type: 'ptt.set'; active: boolean; token?: string }
  | { type: 'driver.ask'; text: string; token?: string }
  | { type: 'voice.setMic'; device: string; token?: string }
  | { type: 'select.car'; carIndex: number };

/** Row from GET /api/sessions/:id/laps */
export interface LapRow {
  carIndex: number;
  driver: string;
  lap: number;
  lapMs: number;
  s1Ms: number | null;
  s2Ms: number | null;
  s3Ms: number | null;
  compound: string | null;
  tyreAge: number | null;
  wear: Wheels<number> | null;
  fuelKg: number | null;
  ersHarvestJ: number | null;
  ersDeployJ: number | null;
  valid: boolean;
  position: number;
  sessionTime: number;
}

export type MonitorId = 'timing' | 'track' | 'tyres' | 'power' | 'damage' | 'weather' | 'comms';
```

### 5.3 REST Endpoints

| Method | Path | Purpose |
| :--- | :--- | :--- |
| `GET` | `/api/health` | Daemon, UDP, TTS, audio and DB status, plus the current session id. |
| `GET` | `/api/snapshot` | The latest telemetry snapshot (same shape as the WebSocket `data`). |
| `GET` | `/api/sessions` | Recorded sessions (most recent first). |
| `GET` | `/api/sessions/current` | `{ id }` of the live session. |
| `GET` | `/api/sessions/:id/laps` | Per-car lap rows for inspector history charts. |
| `GET` | `/api/sessions/:id/radio` | Radio audit log for the session. |
| `GET` | `/api/track-outline/:trackId` | Cached circuit polyline (`404` until a clean lap is recorded). |
| `DELETE` | `/api/track-outline/:trackId` | Forget a cached outline (token-protected). |
| `POST` | `/api/radio` | Send a manual radio message (token in body or `x-pitwall-token` header). |
