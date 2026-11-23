import type { AlertLevel } from './types';

export function lapTime(ms: number | null | undefined): string {
  if (ms == null || ms <= 0) return '—';
  const m = Math.floor(ms / 60000);
  const s = (ms % 60000) / 1000;
  return m > 0 ? `${m}:${s.toFixed(3).padStart(6, '0')}` : s.toFixed(3);
}

export function sector(ms: number | null | undefined): string {
  if (ms == null || ms <= 0) return '—';
  return (ms / 1000).toFixed(3);
}

export function gap(ms: number | null | undefined, prefix = '+'): string {
  if (ms == null) return '—';
  if (ms >= 60000) return `${prefix}${Math.floor(ms / 60000)}:${((ms % 60000) / 1000).toFixed(1).padStart(4, '0')}`;
  return `${prefix}${(ms / 1000).toFixed(ms < 10000 ? 3 : 1)}`;
}

export function signed(v: number | null | undefined, digits = 1, unit = ''): string {
  if (v == null || Number.isNaN(v)) return '—';
  return `${v > 0 ? '+' : v < 0 ? '−' : '±'}${Math.abs(v).toFixed(digits)}${unit}`;
}

export function pct(v: number | null | undefined, digits = 0): string {
  return v == null ? '—' : `${v.toFixed(digits)}%`;
}

export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

export function timeOfDay(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export const CORNERS = ['FL', 'FR', 'RL', 'RR'] as const;

export function levelIcon(l: AlertLevel): string {
  return l === 'critical' ? '■' : l === 'watch' ? '▲' : '●';
}

export function levelLabel(l: AlertLevel): string {
  return l === 'critical' ? 'CRITICAL' : l === 'watch' ? 'WATCH' : 'NOMINAL';
}

/** Wear colour ramp: teal → amber → red. */
export function wearColor(w: number): string {
  if (w >= 65) return 'var(--crit)';
  if (w >= 50) return 'var(--watch)';
  if (w >= 30) return 'var(--warm)';
  return 'var(--ok)';
}

export function tempColor(t: number, cold = 80, hot = 110): string {
  if (t >= hot) return 'var(--crit)';
  if (t >= hot - 8) return 'var(--watch)';
  if (t < cold) return 'var(--cold)';
  return 'var(--ok)';
}

export const COMPOUND_COLOR: Record<string, string> = {
  S: '#ef4444',
  M: '#facc15',
  H: '#f1f5f9',
  I: '#22c55e',
  W: '#3b82f6'
};

/** Approximate livery colours by F1 team id (0–9, modern games). */
const TEAM_COLORS = ['#27f4d2', '#e8002d', '#3671c6', '#64c4ff', '#229971', '#ff87bc', '#6692ff', '#b6babd', '#ff8000', '#52e252'];
export function teamColor(id: number): string {
  return TEAM_COLORS[id] ?? '#94a3b8';
}

export function weatherIcon(w: string): string {
  switch (w) {
    case 'clear':
      return '☀';
    case 'lightCloud':
      return '🌤';
    case 'overcast':
      return '☁';
    case 'lightRain':
      return '🌦';
    case 'heavyRain':
      return '🌧';
    case 'storm':
      return '⛈';
    default:
      return '·';
  }
}

export function weatherLabel(w: string): string {
  return (
    { clear: 'Clear', lightCloud: 'Light cloud', overcast: 'Overcast', lightRain: 'Light rain', heavyRain: 'Heavy rain', storm: 'Storm' } as Record<string, string>
  )[w] ?? '—';
}
