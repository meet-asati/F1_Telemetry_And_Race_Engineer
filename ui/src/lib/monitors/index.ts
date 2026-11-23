import type { Component } from 'svelte';
import type { AlertLevel, MonitorId, TelemetrySnapshot } from '$lib/types';
import Timing from './Timing.svelte';
import Track from './Track.svelte';
import Tyres from './Tyres.svelte';
import Power from './Power.svelte';
import Damage from './Damage.svelte';
import Weather from './Weather.svelte';
import Comms from './Comms.svelte';

export interface MonitorDef {
  id: MonitorId;
  title: string;
  icon: string;
  component: Component<{ snap: TelemetrySnapshot; mode: 'tile' | 'inspect' }>;
  alert: (s: TelemetrySnapshot) => { level: AlertLevel; reason: string | null };
}

const nominal = { level: 'nominal' as AlertLevel, reason: null };

export const MONITORS: Record<MonitorId, MonitorDef> = {
  timing: {
    id: 'timing',
    title: 'Timing & Gaps',
    icon: '⏱',
    component: Timing,
    alert: (s) => {
      const p = s.timing.entries.find((e) => e.isPlayer);
      if (p && p.penaltiesMs > 0) return { level: 'watch', reason: `+${p.penaltiesMs / 1000}s penalty` };
      if (s.timing.gapBehindMs != null && s.timing.gapBehindMs < 1000) return { level: 'watch', reason: `Car behind ${(s.timing.gapBehindMs / 1000).toFixed(1)}s` };
      return nominal;
    }
  },
  track: {
    id: 'track',
    title: 'Track Map',
    icon: '🗺',
    component: Track,
    alert: (s) => {
      if (s.session.marshalZones.some((z) => z.flag === 'red')) return { level: 'critical', reason: 'Red flag' };
      if (s.session.marshalZones.some((z) => z.flag === 'yellow')) return { level: 'watch', reason: 'Yellow flag' };
      return nominal;
    }
  },
  tyres: { id: 'tyres', title: 'Tyres & Thermal', icon: '◎', component: Tyres, alert: (s) => ({ level: s.tyres.alertLevel, reason: s.tyres.alertReason }) },
  power: {
    id: 'power',
    title: 'Power Unit · ERS · Fuel',
    icon: '⚡',
    component: Power,
    alert: (s) => ({ level: s.powertrain.alertLevel, reason: s.powertrain.alertReason })
  },
  damage: { id: 'damage', title: 'Damage & Brakes', icon: '🛠', component: Damage, alert: (s) => ({ level: s.damage.alertLevel, reason: s.damage.alertReason }) },
  weather: {
    id: 'weather',
    title: 'Weather & Session',
    icon: '☁',
    component: Weather,
    alert: (s) => {
      const sc = s.session.safetyCarStatus;
      if (sc === 'full') return { level: 'critical', reason: 'Safety Car' };
      if (sc === 'virtual') return { level: 'critical', reason: 'VSC' };
      const slick = ['S', 'M', 'H'].includes(s.tyres.compound);
      if (slick && ['lightRain', 'heavyRain', 'storm'].includes(s.session.weather)) return { level: 'critical', reason: 'Raining on slicks' };
      const r = s.session.weatherForecast.find((f) => f.timeOffsetMin > 0 && f.timeOffsetMin <= 15 && f.rainPct >= 50);
      if (r && slick) return { level: 'watch', reason: `Rain ${r.rainPct}% in ${r.timeOffsetMin}′` };
      return nominal;
    }
  },
  comms: {
    id: 'comms',
    title: 'AI Engineer & Copilot',
    icon: '🎙',
    component: Comms,
    alert: (s) => {
      const n = s.comms.pendingSuggestions.length;
      if (n > 0) return { level: 'watch', reason: `${n} awaiting approval` };
      return nominal;
    }
  }
};

export type LayoutPreset = 'full' | 'stint' | 'quali' | 'sc';

/** Grid template areas per preset (12-column stand). */
export const LAYOUTS: Record<LayoutPreset, { label: string; areas: string; rows: string }> = {
  full: {
    label: 'Full Stand',
    areas: `"timing track track tyres" "timing track track power" "timing weather damage comms"`,
    rows: 'repeat(3, minmax(0, 1fr))'
  },
  stint: {
    label: 'Stint & Tyres',
    areas: `"tyres tyres power timing" "tyres tyres power timing" "damage weather comms track"`,
    rows: 'minmax(0, 1fr) minmax(0, 1fr) minmax(0, 0.9fr)'
  },
  quali: {
    label: 'Quali & Pace',
    areas: `"track track track timing" "track track track timing" "tyres power comms timing"`,
    rows: 'minmax(0, 1fr) minmax(0, 1fr) minmax(0, 0.9fr)'
  },
  sc: {
    label: 'SC & Weather',
    areas: `"weather weather timing timing" "weather weather timing timing" "damage track tyres comms"`,
    rows: 'minmax(0, 1fr) minmax(0, 1fr) minmax(0, 0.9fr)'
  }
};
