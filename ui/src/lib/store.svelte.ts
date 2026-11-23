// PitWallStore: one reconnecting WebSocket per window, plus REST helpers and
// short client-side histories for the inspector charts.

import type {
  ClientCommand,
  LapRow,
  MonitorId,
  PitWallEvent,
  RadioEvent,
  ServerMessage,
  TelemetrySnapshot,
  Wheels
} from './types';

export interface TyreSample {
  t: number;
  surface: Wheels<number>;
  inner: Wheels<number>;
  brake: Wheels<number>;
  wear: Wheels<number>;
}

export interface LogEntry {
  atMs: number;
  kind: 'flag' | 'penalty' | 'flashback' | 'session';
  text: string;
  level: 'info' | 'watch' | 'critical';
}

export type LayoutPreset = 'full' | 'stint' | 'quali' | 'sc';

const HISTORY_SAMPLES = 900; // 15 min @ 1 Hz
const LOG_MAX = 300;
const TRACE_SAMPLES = 450; // 30 s @ 15 Hz
const OUTLINE_BINS = 720;

/** Player input sample for the live trace (non-reactive; drawn by canvas loops). */
export interface TraceSample {
  t: number;
  throttle: number;
  brake: number;
  speed: number;
  gear: number;
}

/**
 * Track outline built in the browser from every running car's position,
 * binned by lap distance. With a field of cars it fills in within seconds,
 * long before the server has a complete outline to store.
 */
class LiveOutline {
  private sx = new Float64Array(OUTLINE_BINS);
  private sz = new Float64Array(OUTLINE_BINS);
  private n = new Uint16Array(OUTLINE_BINS);
  filled = 0;

  reset() {
    this.sx.fill(0);
    this.sz.fill(0);
    this.n.fill(0);
    this.filled = 0;
  }

  add(pct: number, x: number, z: number) {
    // The server clamps to 0–100, so exact 0 also covers cars behind the line on the grid.
    if (!(pct > 0 && pct < 100) || (x === 0 && z === 0)) return;
    const i = Math.min(OUTLINE_BINS - 1, Math.floor((pct / 100) * OUTLINE_BINS));
    if (this.n[i] === 0) this.filled++;
    if (this.n[i] < 12) {
      this.sx[i] += x;
      this.sz[i] += z;
      this.n[i]++;
    }
  }

  /** Runs of consecutive filled bins (gaps of up to 3 bins are bridged). */
  segments(): [number, number][][] {
    const out: [number, number][][] = [];
    let cur: [number, number][] = [];
    let gap = 0;
    for (let i = 0; i < OUTLINE_BINS; i++) {
      if (this.n[i] === 0) {
        gap++;
        if (gap > 3 && cur.length) {
          out.push(cur);
          cur = [];
        }
        continue;
      }
      gap = 0;
      cur.push([this.sx[i] / this.n[i], this.sz[i] / this.n[i]]);
    }
    if (cur.length) out.push(cur);
    // Join the wrap-around at the start/finish line.
    if (out.length > 1 && this.n[0] && this.n[OUTLINE_BINS - 1]) out[0] = [...out.pop()!, ...out[0]];
    return out;
  }

  get complete() {
    return this.filled >= OUTLINE_BINS * 0.97;
  }
}

function loadPref(key: string, fallback: string): string {
  try {
    return localStorage.getItem(`pitwall.${key}`) ?? fallback;
  } catch {
    return fallback;
  }
}
function savePref(key: string, value: string) {
  try {
    localStorage.setItem(`pitwall.${key}`, value);
  } catch {
    /* storage unavailable */
  }
}

class PitWallStore {
  snap = $state.raw<TelemetrySnapshot | null>(null);
  connected = $state(false);
  authRequired = $state(false);
  serverVersion = $state('');
  lastMsgAt = $state(0);
  msgRate = $state(0);
  now = $state(Date.now());
  error = $state<string | null>(null);

  radioLog = $state.raw<RadioEvent[]>([]);
  raceLog = $state.raw<LogEntry[]>([]);
  selectedCar = $state<number | null>(null);
  inspecting = $state<MonitorId | null>(null);
  layout = $state<LayoutPreset>(loadPref('layout', 'full') as LayoutPreset);
  token = $state(loadPref('token', ''));

  tyreHist = $state.raw<TyreSample[]>([]);
  gapHist = $state.raw<Map<number, { t: number; gap: number }[]>>(new Map());
  laps = $state.raw<LapRow[]>([]);
  outline = $state.raw<[number, number][] | null>(null);
  outlineTrackId: number | null = null;
  /** Browser-built outline used until the server has one (read by canvas loops). */
  liveOutline = new LiveOutline();
  /** Rolling player input trace (read by canvas loops). */
  trace: TraceSample[] = [];

  private ws: WebSocket | null = null;
  private retry = 0;
  private msgCount = 0;
  private lastSampleAt = 0;
  private lapsTimer: ReturnType<typeof setTimeout> | null = null;
  private bc: BroadcastChannel | null = null;
  private started = false;

  start() {
    if (this.started || typeof window === 'undefined') return;
    this.started = true;
    this.connect();
    setInterval(() => {
      this.now = Date.now();
    }, 250);
    setInterval(() => {
      this.msgRate = this.msgCount;
      this.msgCount = 0;
    }, 1000);
    try {
      this.bc = new BroadcastChannel('pitwall');
      this.bc.onmessage = (e) => {
        if (e.data?.type === 'select') this.selectedCar = e.data.carIndex;
      };
    } catch {
      /* BroadcastChannel unsupported */
    }
    this.fetchLaps();
  }

  // ---------------------------------------------------------------- state

  get stale(): boolean {
    if (!this.connected || !this.snap) return true;
    if (this.now - this.lastMsgAt > 2000) return true;
    const age = this.snap.lastPacketAgeMs;
    return age == null || age > 2000;
  }

  get waitingForGame(): boolean {
    return this.connected && (!this.snap || this.snap.lastPacketAgeMs == null);
  }

  get player() {
    const s = this.snap;
    return s?.timing.entries.find((e) => e.isPlayer) ?? null;
  }

  select(carIndex: number | null) {
    this.selectedCar = carIndex;
    this.bc?.postMessage({ type: 'select', carIndex });
  }

  setLayout(l: LayoutPreset) {
    this.layout = l;
    savePref('layout', l);
  }

  setToken(t: string) {
    this.token = t;
    savePref('token', t);
  }

  // ------------------------------------------------------------ websocket

  private connect() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    this.ws = ws;
    ws.onopen = () => {
      this.connected = true;
      this.retry = 0;
      this.error = null;
    };
    ws.onclose = () => {
      this.connected = false;
      this.ws = null;
      const delay = Math.min(5000, 500 * 2 ** this.retry++);
      setTimeout(() => this.connect(), delay);
    };
    ws.onerror = () => ws.close();
    ws.onmessage = (m) => {
      this.lastMsgAt = Date.now();
      this.msgCount++;
      let msg: ServerMessage;
      try {
        msg = JSON.parse(m.data);
      } catch {
        return;
      }
      switch (msg.type) {
        case 'hello':
          this.authRequired = msg.authRequired;
          this.serverVersion = msg.serverVersion;
          break;
        case 'snapshot':
          this.onSnapshot(msg.data);
          break;
        case 'event':
          this.onEvent(msg.event);
          break;
        case 'error':
          this.error = msg.message;
          setTimeout(() => {
            if (this.error === msg.message) this.error = null;
          }, 6000);
          break;
      }
    };
  }

  send(cmd: ClientCommand): boolean {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      this.error = 'Not connected to the pit wall server';
      return false;
    }
    const withToken = cmd.type === 'select.car' || !this.token ? cmd : { ...cmd, token: this.token };
    this.ws.send(JSON.stringify(withToken));
    return true;
  }

  /** Push-to-talk through the pit-wall PC's microphone (hold = true). */
  ptt(active: boolean) {
    return this.send({ type: 'ptt.set', active });
  }

  /** Ask the engineer a question as if the driver said it on the radio. */
  ask(text: string) {
    const t = text.trim();
    return t ? this.send({ type: 'driver.ask', text: t }) : false;
  }

  radio(text: string, attachments: string[] = []) {
    const t = text.trim();
    if (!t) return false;
    return this.send({ type: 'radio.send', text: t, attachments });
  }

  // -------------------------------------------------------------- ingest

  private onSnapshot(s: TelemetrySnapshot) {
    const prev = this.snap;
    this.snap = s;
    if (prev && prev.sessionUid !== s.sessionUid) {
      this.tyreHist = [];
      this.gapHist = new Map();
      this.laps = [];
      this.trace = [];
      this.fetchLaps();
    }
    if (prev?.session.trackId !== s.session.trackId || prev?.sessionUid !== s.sessionUid) {
      this.liveOutline.reset();
    }
    if (prev && prev.session.trackId !== s.session.trackId) {
      this.outline = null;
      this.outlineTrackId = null;
    }
    if (!this.outline) {
      for (const c of s.track.cars) if (!c.inPit) this.liveOutline.add(c.lapDistancePct, c.x, c.z);
    }
    const me = s.track.cars.find((c) => c.isPlayer);
    if (me && s.lastPacketAgeMs != null && s.lastPacketAgeMs < 2000) {
      if (this.trace.length >= TRACE_SAMPLES) this.trace.shift();
      this.trace.push({ t: performance.now(), throttle: me.throttlePct, brake: me.brakePct, speed: me.speedKph, gear: me.gear });
    }
    if (s.track.outlineAvailable && s.session.trackId !== this.outlineTrackId) {
      this.outlineTrackId = s.session.trackId;
      this.fetchOutline(s.session.trackId);
    }
    const t = Date.now();
    if (t - this.lastSampleAt >= 1000 && s.lastPacketAgeMs != null && s.lastPacketAgeMs < 2000) {
      this.lastSampleAt = t;
      const ty = s.tyres;
      this.tyreHist = [...this.tyreHist.slice(-HISTORY_SAMPLES + 1), { t, surface: ty.surfaceTempC, inner: ty.innerTempC, brake: ty.brakeTempC, wear: ty.wearPct }];
      const gh = new Map(this.gapHist);
      for (const e of s.timing.entries) {
        const g = e.position === 1 ? 0 : e.gapToLeaderMs;
        if (g == null) continue;
        const arr = gh.get(e.carIndex) ?? [];
        gh.set(e.carIndex, [...arr.slice(-HISTORY_SAMPLES + 1), { t, gap: g }]);
      }
      this.gapHist = gh;
    }
  }

  private pushLog(e: LogEntry) {
    this.raceLog = [e, ...this.raceLog].slice(0, LOG_MAX);
  }

  private onEvent(e: PitWallEvent) {
    switch (e.kind) {
      case 'radio':
        this.radioLog = [e, ...this.radioLog].slice(0, LOG_MAX);
        break;
      case 'flag': {
        const text = {
          sc: 'Safety Car deployed',
          vsc: 'Virtual Safety Car deployed',
          scEnding: 'Safety Car in this lap',
          vscEnding: 'VSC ending',
          red: 'Red flag',
          green: 'Green flag — track clear',
          chequered: 'Chequered flag'
        }[e.status];
        const level = e.status === 'green' || e.status === 'chequered' ? 'info' : e.status === 'red' || e.status === 'sc' ? 'critical' : 'watch';
        this.pushLog({ atMs: Date.now(), kind: 'flag', text, level });
        break;
      }
      case 'penalty':
        this.pushLog({
          atMs: Date.now(),
          kind: 'penalty',
          text: `${e.driverLabel}: ${e.description}${e.timeSec ? ` (${e.timeSec}s)` : ''} · lap ${e.lap}`,
          level: e.carIndex === this.snap?.timing.playerIndex ? 'critical' : 'watch'
        });
        break;
      case 'flashback':
        this.pushLog({ atMs: Date.now(), kind: 'flashback', text: `Flashback to lap ${e.toLap}`, level: 'watch' });
        this.scheduleLaps(500);
        break;
      case 'session.reset':
        this.pushLog({ atMs: Date.now(), kind: 'session', text: 'New session started', level: 'info' });
        this.radioLog = [];
        break;
      case 'lap.completed':
        this.scheduleLaps(1500);
        break;
    }
  }

  // ------------------------------------------------------------------ REST

  private scheduleLaps(ms: number) {
    if (this.lapsTimer) return;
    this.lapsTimer = setTimeout(() => {
      this.lapsTimer = null;
      this.fetchLaps();
    }, ms);
  }

  async fetchLaps() {
    try {
      const cur = await fetch('/api/sessions/current').then((r) => r.json());
      if (cur?.id == null) return;
      const r = await fetch(`/api/sessions/${cur.id}/laps`);
      if (r.ok) this.laps = await r.json();
      if (this.radioLog.length === 0) await this.fetchRadio(cur.id);
    } catch {
      /* server not ready */
    }
  }

  /** Backfill the radio audit log from the database (newest first). */
  async fetchRadio(sessionId: number) {
    try {
      const r = await fetch(`/api/sessions/${sessionId}/radio`);
      if (!r.ok) return;
      const rows: { atMs: number; priority: RadioEvent["priority"]; source: RadioEvent["source"]; text: string; context: string | null; outcome: RadioEvent["outcome"] }[] = await r.json();
      const hist: RadioEvent[] = rows.map((x) => ({ kind: "radio", atMs: x.atMs, priority: x.priority, source: x.source, text: x.text, outcome: x.outcome, attachments: x.context ? x.context.split("; ") : [] }));
      // Keep anything that arrived live while we were fetching.
      const seen = new Set(this.radioLog.map((e) => `${e.atMs}|${e.text}`));
      this.radioLog = [...this.radioLog, ...hist.reverse().filter((e) => !seen.has(`${e.atMs}|${e.text}`))].slice(0, LOG_MAX);
    } catch {
      /* ignore */
    }
  }

  async fetchOutline(trackId: number) {
    try {
      const r = await fetch(`/api/track-outline/${trackId}`);
      if (!r.ok) return;
      const o = await r.json();
      this.outline = o.points;
    } catch {
      /* ignore */
    }
  }
}

export const pitwall = new PitWallStore();
