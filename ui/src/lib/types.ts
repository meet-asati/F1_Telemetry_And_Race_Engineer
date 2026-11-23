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
  speaking: boolean;
  lastSpokenLine: string | null;
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
