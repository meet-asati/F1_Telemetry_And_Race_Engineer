<script lang="ts" module>
  export type ColorMode = 'positions' | 'speed' | 'inputs' | 'ers';
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { pitwall } from '$lib/store.svelte';
  import { teamColor } from '$lib/format';
  import type { TrackCar } from '$lib/types';

  let { interactive = false, colorMode = 'positions' as ColorMode, labels = false }: { interactive?: boolean; colorMode?: ColorMode; labels?: boolean } = $props();

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;

  // Interpolation: previous and current snapshot positions per car.
  let prev = new Map<number, { x: number; z: number }>();
  let cur = new Map<number, { x: number; z: number }>();
  let lastSnapAt = 0;
  let snapInterval = 66;
  let lastSnap: unknown = null;
  let lastSession: string | null = null;

  // View transform (zoom/pan) for the full view.
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let dragging: { x: number; y: number; px: number; py: number } | null = null;

  // Bounds are kept sticky so the map does not rescale every frame while it fills in.
  let bounds = { x0: Infinity, x1: -Infinity, z0: Infinity, z1: -Infinity };

  function ingest() {
    const s = pitwall.snap;
    if (!s || s === lastSnap) return;
    lastSnap = s;
    if (s.sessionUid !== lastSession) {
      lastSession = s.sessionUid;
      prev.clear();
      cur.clear();
      bounds = { x0: Infinity, x1: -Infinity, z0: Infinity, z1: -Infinity };
    }
    const now = performance.now();
    if (lastSnapAt) snapInterval = Math.min(250, Math.max(30, now - lastSnapAt));
    lastSnapAt = now;
    prev = cur;
    cur = new Map();
    for (const c of s.track.cars) {
      if (c.x === 0 && c.z === 0) continue;
      cur.set(c.carIndex, { x: c.x, z: c.z });
      grow(c.x, c.z);
    }
  }

  function grow(x: number, z: number) {
    if (x < bounds.x0) bounds.x0 = x;
    if (x > bounds.x1) bounds.x1 = x;
    if (z < bounds.z0) bounds.z0 = z;
    if (z > bounds.z1) bounds.z1 = z;
  }

  /** Track geometry to draw: the stored outline, else the live one being built. */
  function geometry(): { closed: boolean; runs: [number, number][][] } {
    const o = pitwall.outline;
    if (o && o.length > 10) return { closed: true, runs: [o] };
    const runs = pitwall.liveOutline.segments();
    return { closed: pitwall.liveOutline.complete && runs.length === 1, runs };
  }

  /** World (x, z) → canvas pixels. The game's +Z points up the map, so it is negated. */
  function transform(w: number, h: number) {
    const b = bounds;
    const spanX = b.x1 - b.x0 || 1;
    const spanZ = b.z1 - b.z0 || 1;
    const margin = labels ? 34 : 18;
    const scale = Math.min((w - margin * 2) / spanX, (h - margin * 2) / spanZ) * zoom;
    const cx = (b.x0 + b.x1) / 2;
    const cz = (b.z0 + b.z1) / 2;
    return {
      tx: (x: number) => w / 2 + (x - cx) * scale + panX,
      tz: (z: number) => h / 2 - (z - cz) * scale + panY
    };
  }

  function carColor(c: TrackCar, teamId: number): string {
    switch (colorMode) {
      case 'speed': {
        const f = Math.min(1, c.speedKph / 330);
        return `hsl(${200 - f * 200}, 90%, 58%)`;
      }
      case 'inputs':
        return c.brakePct > 10 ? '#ff3b3b' : c.throttlePct > 85 ? '#4dfa9a' : '#fbbf24';
      case 'ers':
        return ({ None: '#64748b', Medium: '#38bdf8', Hotlap: '#a78bfa', Overtake: '#f472b6' } as Record<string, string>)[c.ersDeployMode ?? ''] ?? '#475569';
      default:
        return teamColor(teamId);
    }
  }

  function strokeRuns(ctx: CanvasRenderingContext2D, runs: [number, number][][], closed: boolean, t: ReturnType<typeof transform>) {
    ctx.beginPath();
    for (const run of runs) {
      run.forEach(([x, z], i) => (i ? ctx.lineTo(t.tx(x), t.tz(z)) : ctx.moveTo(t.tx(x), t.tz(z))));
      if (closed) ctx.closePath();
    }
    ctx.stroke();
  }

  function draw() {
    ingest();
    const s = pitwall.snap;
    const dpr = window.devicePixelRatio || 1;
    const w = wrap.clientWidth;
    const h = wrap.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const geo = geometry();
    for (const run of geo.runs) for (const [x, z] of run) grow(x, z);
    if (!Number.isFinite(bounds.x0)) {
      ctx.fillStyle = '#4b675d';
      ctx.font = '600 11px Bahnschrift, sans-serif';
      ctx.textAlign = 'center';
      ctx.fillText(s ? 'ACQUIRING TRACK…' : 'NO TELEMETRY', w / 2, h / 2);
      return;
    }
    const t = transform(w, h);
    const { tx, tz } = t;

    // Track: dark tarmac band, then a glowing centre line.
    if (geo.runs.length) {
      ctx.lineJoin = 'round';
      ctx.lineCap = 'round';
      ctx.strokeStyle = 'rgba(160, 200, 210, 0.13)';
      ctx.lineWidth = labels ? 14 : 10;
      strokeRuns(ctx, geo.runs, geo.closed, t);
      ctx.save();
      ctx.shadowColor = 'rgba(190, 240, 255, 0.8)';
      ctx.shadowBlur = 8;
      ctx.strokeStyle = '#d4e6ec';
      ctx.lineWidth = 2;
      strokeRuns(ctx, geo.runs, geo.closed, t);
      ctx.restore();

      // Marshal-zone flags over the outline (zones are fractions of lap distance).
      const o = geo.closed ? geo.runs[0] : null;
      const zones = s?.session.marshalZones ?? [];
      if (o) {
        zones.forEach((zn, i) => {
          if (zn.flag !== 'yellow' && zn.flag !== 'red') return;
          const end = i + 1 < zones.length ? zones[i + 1].startFraction : 1;
          const a = Math.floor(zn.startFraction * o.length);
          const e = Math.max(a + 1, Math.floor(end * o.length));
          ctx.save();
          ctx.strokeStyle = zn.flag === 'red' ? '#ff3b3b' : '#facc15';
          ctx.shadowColor = ctx.strokeStyle;
          ctx.shadowBlur = 10;
          ctx.lineWidth = 5;
          ctx.beginPath();
          for (let k = a; k < e && k < o.length; k++) {
            const [x, z] = o[k];
            if (k === a) ctx.moveTo(tx(x), tz(z));
            else ctx.lineTo(tx(x), tz(z));
          }
          ctx.stroke();
          ctx.restore();
        });
        // Start/finish line, perpendicular to the track.
        const [sx, sz] = o[0];
        const [nx, nz] = o[1];
        const ang = Math.atan2(tz(nz) - tz(sz), tx(nx) - tx(sx)) + Math.PI / 2;
        ctx.strokeStyle = '#ff3b3b';
        ctx.lineWidth = 3;
        ctx.beginPath();
        ctx.moveTo(tx(sx) - Math.cos(ang) * 9, tz(sz) - Math.sin(ang) * 9);
        ctx.lineTo(tx(sx) + Math.cos(ang) * 9, tz(sz) + Math.sin(ang) * 9);
        ctx.stroke();
      }
    }

    // Cars (interpolated between snapshots).
    if (!s) return;
    const f = Math.min(1, (performance.now() - lastSnapAt) / snapInterval);
    const teams = new Map(s.timing.entries.map((e) => [e.carIndex, e]));
    const ordered = [...s.track.cars].sort((a, c) => Number(a.isPlayer) - Number(c.isPlayer));
    for (const c of ordered) {
      const p1 = cur.get(c.carIndex);
      if (!p1) continue;
      const p0 = prev.get(c.carIndex) ?? p1;
      const jump = Math.hypot(p1.x - p0.x, p1.z - p0.z) > 80;
      const x = jump ? p1.x : p0.x + (p1.x - p0.x) * f;
      const z = jump ? p1.z : p0.z + (p1.z - p0.z) * f;
      const e = teams.get(c.carIndex);
      const selected = pitwall.selectedCar === c.carIndex;
      const r = c.isPlayer ? 6.5 : selected ? 6 : 4.5;
      const col = carColor(c, e?.teamId ?? 255);
      ctx.globalAlpha = c.inPit ? 0.35 : 1;
      ctx.save();
      ctx.shadowColor = col;
      ctx.shadowBlur = 8;
      ctx.beginPath();
      ctx.arc(tx(x), tz(z), r, 0, Math.PI * 2);
      ctx.fillStyle = col;
      ctx.fill();
      ctx.restore();
      ctx.lineWidth = 1;
      ctx.strokeStyle = 'rgba(0,0,0,0.8)';
      ctx.stroke();
      if (c.isPlayer || selected) {
        ctx.lineWidth = 2;
        ctx.strokeStyle = c.isPlayer ? '#ffe14d' : '#ffffff';
        ctx.beginPath();
        ctx.arc(tx(x), tz(z), r + 3.5, 0, Math.PI * 2);
        ctx.stroke();
      }
      if ((labels || c.isPlayer || selected) && e) {
        const text = `${e.position} ${e.driverLabel}`;
        ctx.font = `${c.isPlayer ? 700 : 600} 10px Consolas, ui-monospace, monospace`;
        const tw = ctx.measureText(text).width;
        const lx = tx(x) + r + 5;
        const ly = tz(z) - 7;
        ctx.fillStyle = 'rgba(2, 7, 10, 0.75)';
        ctx.fillRect(lx - 2, ly, tw + 4, 13);
        ctx.fillStyle = c.isPlayer ? '#ffe14d' : '#cfe8dc';
        ctx.textAlign = 'left';
        ctx.fillText(text, lx, ly + 10);
      }
      ctx.globalAlpha = 1;
    }
  }

  onMount(() => {
    let raf = 0;
    const loop = () => {
      draw();
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  });

  function onWheel(e: WheelEvent) {
    if (!interactive) return;
    e.preventDefault();
    zoom = Math.min(8, Math.max(0.5, zoom * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
  }
  function onDown(e: PointerEvent) {
    if (!interactive) return;
    dragging = { x: e.clientX, y: e.clientY, px: panX, py: panY };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onMove(e: PointerEvent) {
    if (!dragging) return;
    panX = dragging.px + e.clientX - dragging.x;
    panY = dragging.py + e.clientY - dragging.y;
  }
  function onUp() {
    dragging = null;
  }

  // Click a car to select it (full view only).
  function onClick(e: MouseEvent) {
    if (!interactive) return;
    const s = pitwall.snap;
    if (!s || !Number.isFinite(bounds.x0)) return;
    const rect = canvas.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const { tx, tz } = transform(rect.width, rect.height);
    let best: { d: number; i: number } | null = null;
    for (const c of s.track.cars) {
      const d = Math.hypot(tx(c.x) - mx, tz(c.z) - my);
      if (d < 14 && (!best || d < best.d)) best = { d, i: c.carIndex };
    }
    if (best) pitwall.select(best.i);
  }
</script>

<div class="wrap" bind:this={wrap} class:interactive>
  <canvas
    bind:this={canvas}
    onwheel={onWheel}
    onpointerdown={onDown}
    onpointermove={onMove}
    onpointerup={onUp}
    onclick={onClick}
    aria-label="Live track map"
  ></canvas>
  {#if interactive && (zoom !== 1 || panX !== 0 || panY !== 0)}
    <button class="reset" onclick={() => ((zoom = 1), (panX = 0), (panY = 0))}>Reset view</button>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 120px;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .interactive canvas {
    cursor: grab;
  }
  .reset {
    position: absolute;
    top: 6px;
    right: 6px;
    font-size: 11px;
  }
</style>
