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
      this.fetchLaps();
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
