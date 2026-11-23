# UI/UX Specification (`UI.md`): F1 22 Multi-Monitor Pit Wall

- **Frontend Stack:** SvelteKit / Vite + TypeScript + HTML5 `<canvas>` (and lightweight SVG/chart primitives)
- **Backend Transport:** WebSocket stream (`15Hz` state snapshots + event messages) + REST endpoints served by Rust `axum` over Tailscale
- **Primary Audience:** Human Race Engineer (Friend on Pit Wall) + optional minimal Driver HUD/Overlay

---

## 1. Design Philosophy & Visual Language

The UI models an **F1 pit-wall monitor stand**: a structured, high-contrast command surface where your friend can scan critical race domains at a glance and click **any monitor** (or metric/car) to open a deep-dive inspection view.

- **Dark Pit-Wall Aesthetic:** Low-glare charcoal/slate backgrounds (`#0B0E14` canvas, `#121722` monitor bezel, `#181F2E` elevated surface) with crisp hairline borders (`1px solid rgba(255,255,255,0.08)`).
- **Tabular Precision:** Use monospace / `font-variant-numeric: tabular-nums` for lap times, gaps, temperatures, wear percentages, and ERS/fuel deltas so numbers never jitter as values tick at `15Hz`.
- **Semantic Alert States (`Nominal` | `Watch` | `Critical`):**
  - **Nominal:** Calm neutral/teal/slate indicators; no visual noise.
  - **Watch:** Amber border highlight + concise status badge (e.g., tyre wear approaching window, slight fuel deficit, rain probability rising).
  - **Critical:** Red/high-contrast border highlight + priority flag (e.g., high tyre wear/temp spike, wing/floor damage, Safety Car / VSC / penalty).
- **Zero-Distraction Rule:** Keep monitor tiles scannable in `< 1.5s`; push dense historical charts, raw F1 22 packet fields, and editing controls into the **Click-to-Inspect** view.

---

## 2. Workspace Layout & Navigation

### 2.1 Top Pit-Wall Bar
- **Session & Connection Strip:** Session type (Practice / Quali / Race), current lap (`Lap X / Y`), track & weather icon/temp, F1 22 UDP live/replay status indicator, and WebSocket latency/connection pill.
- **Layout Presets:** Switchable multi-monitor arrangements:
  - `Full Stand` (default 7-monitor grid)
  - `Stint & Tyres` (emphasizes Tyres, Power/Fuel, and Timing)
  - `Quali & Pace` (emphasizes Track Map, Timing/Deltas, and Comms)
  - `SC & Weather` (emphasizes Weather Radar, Timing, and Damage)
- **Duo Mode & AI Controls:**
  - Toggle **Auto-Speak** vs. **Copilot Approval** mode.
  - Show active **Friend Override** timer badge when the human engineer is speaking on Discord or sending radio messages.
  - Quick mute/unmute indicator for AI radio output.

### 2.2 Multi-Monitor Stand Grid
- **Monitor Tiles:** Each telemetry domain renders as a self-contained monitor card with:
  - Monitor title, domain icon, and `Nominal` / `Watch` / `Critical` badge.
  - Compact high-signal summary metrics (designed for fast scanning).
  - **Click anywhere on the tile** to open the **Monitor Inspector** for that domain.
  - **Pop-Out Button** on each tile header to open that monitor in a standalone browser window (useful for multi-monitor setups).

### 2.3 Click-to-Inspect Drawer / Focus Modal
Clicking a monitor opens a dedicated **Inspector View** for that domain containing:
1. **Full Telemetry Breakdown:** Every relevant F1 22 field for that system (current, previous lap, and rolling averages).
2. **Historical & Comparative Curves:** Lap-by-lap progression, stint comparisons, or per-corner values.
3. **Live / Snapshot Mode Toggle:** Switch between live `15Hz` updates or inspect a specific lap/event from the session.
4. **Quick "Send to Radio" Composer:** Pre-populated context from the inspected metric/driver with one-click send or custom text edit.

---

## 3. Monitor Specifications

### Monitor 1: Timing & Gap Tower
- **Tile Summary:** Compact standings list showing position, driver code/number, gap to car ahead/leader, current tyre compound + lap age, pit count, and active penalty/warning tags.
- **Click-to-Inspect View:**
  - Full 22-car timing table with last lap, best lap, sector 1/2/3 times, pit status, and tyre stint history.
  - Pace delta & gap trend chart against a selected rival or the car ahead.
  - Quick radio buttons (e.g., *"Push to catch car ahead"*, *"Manage gap / defend"*, *"Box next lap"*).

### Monitor 2: Live Track & Sector Map
- **Tile Summary:** `<canvas>` circuit outline generated from F1 22 `Motion` world coordinates (`m_worldPositionX`, `m_worldPositionZ`) with live car dots, highlighted player car, and marshal/yellow-flag sector markers.
- **Click-to-Inspect View:**
  - Zoomable/pan-capable track canvas with color-mode selector (`Positions`, `Speed`, `Throttle/Brake`, `ERS Deploy`).
  - Selected car telemetry readout (speed, gear, throttle/brake bars, ERS deploy state, lap distance %).
  - Corner/sector marker list and instant *"Warn traffic / flag in sector"* radio shortcut.

### Monitor 3: Tyre & Thermal Stand
- **Tile Summary:** 4-corner layout (`FL`, `FR`, `RL`, `RR`) displaying tyre wear `%`, surface/carcass temperature, compound badge (`S`, `M`, `H`, `I`, `W`), and tyre age in laps.
- **Click-to-Inspect View:**
  - Per-corner wear rate (`%` loss per lap) and projected pit-window lap based on current degradation slope.
  - Thermal graph comparing surface vs. inner carcass temperatures and brake temperatures across recent laps.
  - Tyre damage/blister status and quick radio actions (e.g., *"Cool front tyres"*, *"Box for new set"*).

### Monitor 4: Power Unit, ERS & Fuel
- **Tile Summary:** Fuel remaining vs. target (`+laps` / `-laps` delta), ERS battery percentage and current deploy mode (`None`, `Medium`, `Hotlap`, `Overtake`), and quick powertrain health status.
- **Click-to-Inspect View:**
  - Fuel burn per lap vs. required burn to finish, plus estimated lift-and-coast savings.
  - ERS harvest vs. deployment per lap and lap-distance deployment profile.
  - Component wear bars (`ICE`, `MGU-H`, `MGU-K`, `ES`, `CE`, `TC`, gearbox) and quick radio actions (e.g., *"Lift and coast for fuel"*, *"Deploy ERS / overtake mode"*).

### Monitor 5: Car Damage & Chassis/Brakes
- **Tile Summary:** Front wing left/right damage `%`, rear wing damage `%`, floor/diffuser damage `%`, and peak brake temperature alert state.
- **Click-to-Inspect View:**
  - Visual chassis damage diagram highlighting affected aero and floor zones.
  - Front/rear brake temperature readout and current car setup summary (`CarSetups`: brake bias, diff, wing/suspension snapshot).
  - Quick radio actions (e.g., *"Watch brake temps"*, *"Box for front wing repair"*).

### Monitor 6: Weather & Session Radar
- **Tile Summary:** Current weather condition, air/track temperatures, rain forecast indicator, session laps/time remaining, and Safety Car / VSC status.
- **Click-to-Inspect View:**
  - Weather forecast timeline (rain probability and expected weather shifts over upcoming minutes/laps).
  - Session & flag log (Safety Car, VSC, red flags, track limits/penalties).
  - Quick radio actions (e.g., *"Inters expected soon, stay out"*, *"Box for intermediates"*).

### Monitor 7: AI Engineer & Copilot Desk
- **Tile Summary:** Active comms mode (`Auto-Speak` / `Copilot Approval`), pending AI suggestion count, last spoken engineer message, and PTT / wake-word status.
- **Click-to-Inspect View:**
  - **Copilot Approval Queue:** Pending P2 AI suggestions with **Approve & Speak**, **Edit & Send**, and **Dismiss** actions.
  - **Manual Radio Console:** Quick preset buttons + free-text input that routes through the radio TTS pipeline into Discord voice.
  - **Radio & Prompt Audit Log:** Chronological feed of friend messages, AI callouts, driver voice queries, and the telemetry snapshot context used.

---

## 4. Cross-Monitor Interactions & Duo Mode Workflows

- **Click-a-Driver Cross-Highlight:** Selecting a driver in the Timing Tower or Track Map highlights that driver across related views so the race engineer can compare pace, tyres, and gaps at a glance.
- **Context-Attached Radio:** Clicking an alert or metric inside any monitor's inspector lets the friend attach that exact reading (e.g., `FL wear 68%`, `Fuel -0.7 laps`) to a radio message before sending.
- **Copilot Approval vs. Override:**
  - In **Copilot Approval** mode, proactive AI recommendations wait for friend approval before speaking.
  - When the friend sends a radio message or speaks on Discord, non-urgent P2 AI callouts are automatically held/suppressed during the override cooldown window.

---

## 5. WebSocket & Frontend State Contract

The Svelte UI maintains a client-side `PitWallStore` fed by typed messages from Rust `axum`:

```ts
export type AlertLevel = "nominal" | "watch" | "critical";

export interface TelemetrySnapshot {
  timestampMs: number;
  session: {
    trackName: string;
    sessionType: string;
    lap: number;
    totalLaps: number;
    weather: string;
    airTempC: number;
    trackTempC: number;
    rainForecastSummary: string;
    safetyCarStatus: "none" | "full" | "virtual" | "formation";
  };
  timing: {
    playerIndex: number;
    position: number;
    gapAheadSec: number | null;
    gapBehindSec: number | null;
    lastLapMs: number | null;
    bestLapMs: number | null;
    entries: Array<{
      carIndex: number;
      position: number;
      driverLabel: string;
      gapToLeaderSec: number | null;
      compound: string;
      tyreAgeLaps: number;
      pitStops: number;
      penaltiesSec: number;
      inPit: boolean;
    }>;
  };
  track: {
    cars: Array<{
      carIndex: number;
      x: number;
      z: number;
      speedKph: number;
      isPlayer: boolean;
      drsActive: boolean;
    }>;
    yellowSector: number | null;
  };
  tyres: {
    compound: string;
    ageLaps: number;
    wearPct: [number, number, number, number]; // FL, FR, RL, RR
    wearRatePerLap: [number, number, number, number];
    surfaceTempC: [number, number, number, number];
    innerTempC: [number, number, number, number];
    brakeTempC: [number, number, number, number];
    alertLevel: AlertLevel;
  };
  powertrain: {
    fuelRemainingLapsDelta: number;
    fuelRemainingKg: number;
    ersStorePct: number;
    ersDeployMode: string;
    iceWearPct: number;
    gearboxWearPct: number;
    alertLevel: AlertLevel;
  };
  damage: {
    frontLeftWingPct: number;
    frontRightWingPct: number;
    rearWingPct: number;
    floorPct: number;
    alertLevel: AlertLevel;
  };
  comms: {
    mode: "auto" | "copilot";
    friendOverrideActive: boolean;
    pttActive: boolean;
    lastSpokenLine: string | null;
    pendingSuggestions: Array<{
      id: string;
      category: string;
      message: string;
      createdAtMs: number;
    }>;
  };
}
