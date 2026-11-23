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

- **Dark Pit-Wall Aesthetic.**
  - Low-glare charcoal/slate backgrounds: `#0B0E14` canvas, `#121722` monitor bezel, `#181F2E` elevated surface.
  - Crisp hairline borders: `1px solid rgba(255,255,255,0.08)`.
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
  - the monitor title, a domain icon and the `Nominal` / `Watch` / `Critical` badge;
  - compact, high-signal summary metrics designed for fast scanning.
- **Click anywhere on the tile** to open the **Monitor Inspector** for that domain.
- **Pop-Out Button** on each tile header opens that monitor in a standalone browser window (`/monitor/:id`), which is useful for multi-monitor setups.
  - Pop-outs share the selected car and the layout state with the main window via `BroadcastChannel`.
  - Each window keeps its own WebSocket.

### 2.3 Click-to-Inspect Drawer / Focus Modal

Clicking a monitor opens a dedicated **Inspector View** for that domain, containing:

1. **Full Telemetry Breakdown:** Every relevant field for that system, showing the current value, the previous lap and rolling averages.
2. **Historical & Comparative Curves:** Lap-by-lap progression, stint comparisons or per-corner values. History comes from `GET /api/sessions/:id/laps`.
3. **Live / Snapshot Mode Toggle:** Switch between live updates and inspecting a specific lap or event from the session.
4. **Quick "Send to Radio" Composer:** Pre-populated with context from the inspected metric or driver, with one-click send or custom text editing.

### 2.4 Driver View (`/driver`)

- A large-type, glanceable single screen for a phone, tablet or second monitor:
  - position, gap ahead/behind, last lap vs. best, fuel delta, ERS %, worst tyre wear;
  - the last radio line;
  - the Safety Car / VSC banner.
- No controls beyond mute. (An in-game overlay on top of a fullscreen game isn't practical, so it is out of scope.)

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
  - A `<canvas>` circuit outline built from Motion world coordinates (`worldPositionX`, `worldPositionZ`; Y is up).
  - Live car dots, with the player car highlighted.
  - Per-zone flag markers from `marshalZones[]`.
- **Outline source:**
  - The outline comes from the player's first clean lap and is cached per track (`GET /api/track-outline/:trackId`).
  - Until an outline exists, the map draws accumulated car positions as a faint point cloud.
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
  - Per-corner tyre damage `%`.
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
  - The last spoken engineer message.
  - PTT / wake-word status and TTS sidecar health.
- **Click-to-Inspect View:**
  - **Copilot Approval Queue.** Pending P2 suggestions, each with:
    - a priority badge and an expiry countdown;
    - **Approve & Speak**, **Edit & Send** and **Dismiss** actions.
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

### 5.1 Server → Client Messages

```ts
export type AlertLevel = "nominal" | "watch" | "critical";
export type Wheels<T> = [T, T, T, T]; // FL, FR, RL, RR
export type DataSource = "live" | "replay" | "synthetic" | "none";

export type ServerMessage =
  | { type: "hello"; schemaVersion: number; serverVersion: string; authRequired: boolean }
  | { type: "snapshot"; seq: number; data: TelemetrySnapshot }
  | { type: "event"; event: PitWallEvent };

export interface TelemetrySnapshot {
  schemaVersion: number;
  timestampMs: number;
  source: DataSource;
  gameYear: number | null;           // 22 | 23 | 24 | 25
  sessionUid: string | null;

  session: {
    trackId: number;
    trackName: string;
    sessionType: string;             // "P1" | "Q3" | "Race" | "TimeTrial" ...
    lap: number;
    totalLaps: number;
    timeLeftMs: number;
    weather: string;                 // "clear" | "lightCloud" | "overcast" | "lightRain" | "heavyRain" | "storm"
    airTempC: number;
    trackTempC: number;
    forecastAccuracy: "perfect" | "approximate";
    weatherForecast: Array<{
      timeOffsetMin: number;
      weather: string;
      rainPct: number;
      airTempC: number;
      trackTempC: number;
    }>;
    marshalZones: Array<{ startFraction: number; flag: "none" | "green" | "blue" | "yellow" | "red" }>;
    safetyCarStatus: "none" | "full" | "virtual" | "formation";
    pitWindow: { idealLap: number; latestLap: number; rejoinPosition: number } | null;
  };

  timing: {
    playerIndex: number;
    position: number;
    gapAheadMs: number | null;
    gapBehindMs: number | null;
    lastLapMs: number | null;
    bestLapMs: number | null;
    entries: Array<{
      carIndex: number;
      position: number;
      driverLabel: string;           // 3-letter code or name
      raceNumber: number;
      teamId: number;
      isPlayer: boolean;
      gapToLeaderMs: number | null;
      gapToAheadMs: number | null;
      lastLapMs: number | null;
      bestLapMs: number | null;
      sectorsMs: [number | null, number | null, number | null]; // current/last lap S1, S2, S3
      compound: string;              // visual: "S" | "M" | "H" | "I" | "W"
      tyreAgeLaps: number | null;
      pitStops: number;
      pitStatus: "none" | "pitting" | "inPitArea";
      penaltiesMs: number;
      warnings: number;
      driverStatus: "garage" | "flyingLap" | "inLap" | "outLap" | "onTrack";
      resultStatus: "active" | "finished" | "dnf" | "dsq" | "retired" | "notClassified" | "inactive";
      telemetryRestricted: boolean;
      stints: Array<{ compound: string; startLap: number; endLap: number | null }>;
    }>;
  };

  track: {
    cars: Array<{
      carIndex: number;
      x: number;
      z: number;
      lapDistancePct: number;
      speedKph: number;
      gear: number;
      throttlePct: number;
      brakePct: number;
      ersDeployMode: string | null;
      isPlayer: boolean;
      drsActive: boolean;
    }>;
  };

  tyres: {                           // player car
    compound: string;
    actualCompound: string;          // "C1".."C5" | "Inter" | "Wet"
    ageLaps: number;
    wearPct: Wheels<number>;
    wearRatePerLap: Wheels<number | null>;
    damagePct: Wheels<number>;
    surfaceTempC: Wheels<number>;
    innerTempC: Wheels<number>;
    brakeTempC: Wheels<number>;
    projectedPitLap: number | null;
    alertLevel: AlertLevel;
    alertReason: string | null;
  };

  powertrain: {                      // player car
    fuelRemainingLapsDelta: number;  // MFD value
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
  };

  damage: {                          // player car
    frontLeftWingPct: number;
    frontRightWingPct: number;
    rearWingPct: number;
    floorPct: number;
    diffuserPct: number;
    sidepodPct: number;
    brakeDamagePct: Wheels<number>;
    drsFault: boolean;
    alertLevel: AlertLevel;
    alertReason: string | null;
  };

  comms: {
    mode: "auto" | "copilot";
    muted: boolean;
    friendOverrideActive: boolean;
    overrideRemainingMs: number;
    pttActive: boolean;
    ttsAvailable: boolean;
    lastSpokenLine: string | null;
    pendingSuggestions: Array<{
      id: string;
      priority: "P0" | "P1" | "P2";
      category: string;              // "tyres" | "fuel" | "weather" | "gap" | "damage" | ...
      message: string;
      createdAtMs: number;
      expiresAtMs: number;
      contextAttachments: string[];  // e.g. ["FL wear 68%", "Fuel -0.7 laps"]
    }>;
  };
}

export type PitWallEvent =
  | { kind: "radio"; atMs: number; source: "rule" | "llm" | "friend" | "driver"; priority: "P0" | "P1" | "P2"; text: string; outcome: "spoken" | "queued" | "dismissed" | "expired" | "textOnly" }
  | { kind: "flag"; atMs: number; status: "sc" | "vsc" | "vscEnding" | "scEnding" | "red" | "green"; }
  | { kind: "penalty"; atMs: number; carIndex: number; description: string; timeSec: number | null }
  | { kind: "suggestion.added"; suggestionId: string }
  | { kind: "suggestion.resolved"; suggestionId: string; outcome: "approved" | "edited" | "dismissed" | "expired" }
  | { kind: "session.reset"; sessionUid: string }
  | { kind: "flashback"; atMs: number; toLap: number }
  | { kind: "lap.completed"; carIndex: number; lap: number; lapMs: number };
```

### 5.2 Client → Server Commands

If the daemon has a shared token configured, every mutating command must include `token`.

```ts
export type ClientCommand =
  | { type: "radio.send"; text: string; attachments?: string[]; token?: string }
  | { type: "suggestion.approve"; id: string; token?: string }
  | { type: "suggestion.edit"; id: string; text: string; token?: string }
  | { type: "suggestion.dismiss"; id: string; token?: string }
  | { type: "comms.setMode"; mode: "auto" | "copilot"; token?: string }
  | { type: "comms.mute"; muted: boolean; token?: string }
  | { type: "select.car"; carIndex: number };   // UI-local hint, no token required
```

### 5.3 REST Endpoints

| Method | Path | Purpose |
| :--- | :--- | :--- |
| `GET` | `/api/health` | Daemon, UDP and sidecar status. |
| `GET` | `/api/sessions` | Recorded sessions (most recent first). |
| `GET` | `/api/sessions/:id/laps` | Per-car lap rows for inspector history charts. |
| `GET` | `/api/track-outline/:trackId` | Cached circuit polyline (`404` until a clean lap is recorded). |
| `POST` | `/api/radio` | Send a manual radio message (token-protected when configured). |
