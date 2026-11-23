<script lang="ts" module>
  export type ColorMode = "positions" | "speed" | "inputs" | "ers";
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { pitwall } from '$lib/store.svelte';
  import { teamColor } from '$lib/format';
  import type { TrackCar } from '$lib/types';

  let { interactive = false, colorMode = 'positions' as ColorMode, labels = false }: { interactive?: boolean; colorMode?: ColorMode; labels?: boolean } = $props();

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;

  // Client-side point cloud used until the server has an outline for this track.
  let cloud: [number, number][] = [];
  let cloudSession: string | null = null;

  // Interpolation: previous and current snapshot positions per car.
  let prev = new Map<number, { x: number; z: number }>();
  let cur = new Map<number, { x: number; z: number }>();
  let lastSnapAt = 0;
  let snapInterval = 66;
  let lastSnap: unknown = null;

  // View transform (zoom/pan) for the inspector.
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let dragging: { x: number; y: number; px: number; py: number } | null = null;

  function ingest() {
    const s = pitwall.snap;
    if (!s || s === lastSnap) return;
    lastSnap = s;
    if (s.sessionUid !== cloudSession) {
      cloudSession = s.sessionUid;
      cloud = [];
      prev.clear();
      cur.clear();
    }
    const now = performance.now();
    if (lastSnapAt) snapInterval = Math.min(250, Math.max(30, now - lastSnapAt));
    lastSnapAt = now;
    prev = cur;
    cur = new Map();
    for (const c of s.track.cars) {
      cur.set(c.carIndex, { x: c.x, z: c.z });
      if (!pitwall.outline && !c.inPit && cloud.length < 6000) {
        const last = cloud[cloud.length - 1];
        if (!last || Math.hypot(last[0] - c.x, last[1] - c.z) > 12) cloud.push([c.x, c.z]);
      }
    }
  }

  function bounds(pts: [number, number][]) {
    let x0 = Infinity,
      x1 = -Infinity,
      z0 = Infinity,
      z1 = -Infinity;
    for (const [x, z] of pts) {
      if (x < x0) x0 = x;
      if (x > x1) x1 = x;
      if (z < z0) z0 = z;
      if (z > z1) z1 = z;
    }
    return { x0, x1, z0, z1 };
  }

  function carColor(c: TrackCar, teamId: number): string {
    switch (colorMode) {
      case 'speed': {
        const f = Math.min(1, c.speedKph / 330);
        return `hsl(${200 - f * 200}, 85%, 55%)`;
      }
      case 'inputs':
        return c.brakePct > 10 ? '#ef4444' : c.throttlePct > 85 ? '#22c55e' : '#eab308';
      case 'ers':
        return ({ None: '#64748b', Medium: '#38bdf8', Hotlap: '#a78bfa', Overtake: '#f472b6' } as Record<string, string>)[c.ersDeployMode ?? ''] ?? '#475569';
      default:
        return teamColor(teamId);
    }
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

    const outline = pitwall.outline;
    const ref = outline && outline.length > 10 ? outline : cloud;
    if (ref.length < 3) {
      ctx.fillStyle = '#5d6a80';
      ctx.font = '12px system-ui';
      ctx.textAlign = 'center';
      ctx.fillText(s ? 'Building track map…' : 'No telemetry', w / 2, h / 2);
      return;
    }
    const b = bounds(ref);
    const margin = 18;
    const scale = Math.min((w - margin * 2) / (b.x1 - b.x0 || 1), (h - margin * 2) / (b.z1 - b.z0 || 1)) * zoom;
    const cx = (b.x0 + b.x1) / 2;
    const cz = (b.z0 + b.z1) / 2;
    const tx = (x: number) => w / 2 + (x - cx) * scale + panX;
    const tz = (z: number) => h / 2 + (z - cz) * scale + panY;

    // Track.
    if (outline && outline.length > 10) {
      ctx.lineJoin = 'round';
      ctx.lineCap = 'round';
      ctx.strokeStyle = '#2a3447';
      ctx.lineWidth = 9;
      ctx.beginPath();
      outline.forEach(([x, z], i) => (i ? ctx.lineTo(tx(x), tz(z)) : ctx.moveTo(tx(x), tz(z))));
      ctx.closePath();
      ctx.stroke();
      ctx.strokeStyle = '#3b4760';
      ctx.lineWidth = 1.5;
      ctx.stroke();
      // Marshal zone flags over the outline (zone start fractions of lap distance).
      const zones = s?.session.marshalZones ?? [];
      zones.forEach((zn, i) => {
        if (zn.flag !== 'yellow' && zn.flag !== 'red') return;
        const end = i + 1 < zones.length ? zones[i + 1].startFraction : 1;
        const a = Math.floor(zn.startFraction * outline.length);
        const e = Math.max(a + 1, Math.floor(end * outline.length));
        ctx.strokeStyle = zn.flag === 'red' ? '#ef4444' : '#facc15';
        ctx.lineWidth = 5;
        ctx.beginPath();
        for (let k = a; k < e && k < outline.length; k++) {
          const [x, z] = outline[k];
          k === a ? ctx.moveTo(tx(x), tz(z)) : ctx.lineTo(tx(x), tz(z));
        }
        ctx.stroke();
      });
      // Start/finish line.
      const [sx, sz] = outline[0];
      ctx.fillStyle = '#e6ebf3';
      ctx.fillRect(tx(sx) - 2, tz(sz) - 6, 4, 12);
    } else {
      ctx.fillStyle = 'rgba(148,163,184,0.25)';
      for (const [x, z] of cloud) ctx.fillRect(tx(x) - 1, tz(z) - 1, 2, 2);
    }

    // Cars (interpolated between snapshots).
    if (!s) return;
    const f = Math.min(1, (performance.now() - lastSnapAt) / snapInterval);
    const teams = new Map(s.timing.entries.map((e) => [e.carIndex, e]));
    const ordered = [...s.track.cars].sort((a, c) => Number(a.isPlayer) - Number(c.isPlayer));
    for (const c of ordered) {
      const p0 = prev.get(c.carIndex) ?? cur.get(c.carIndex)!;
      const p1 = cur.get(c.carIndex)!;
      if (!p1) continue;
      const jump = Math.hypot(p1.x - p0.x, p1.z - p0.z) > 80;
      const x = jump ? p1.x : p0.x + (p1.x - p0.x) * f;
      const z = jump ? p1.z : p0.z + (p1.z - p0.z) * f;
      const e = teams.get(c.carIndex);
      const selected = pitwall.selectedCar === c.carIndex;
      const r = c.isPlayer ? 7 : selected ? 6 : 4.5;
      ctx.globalAlpha = c.inPit ? 0.35 : 1;
      ctx.beginPath();
      ctx.arc(tx(x), tz(z), r, 0, Math.PI * 2);
      ctx.fillStyle = carColor(c, e?.teamId ?? 255);
      ctx.fill();
      if (c.isPlayer || selected) {
        ctx.lineWidth = 2;
        ctx.strokeStyle = c.isPlayer ? '#fde047' : '#ffffff';
        ctx.beginPath();
        ctx.arc(tx(x), tz(z), r + 3, 0, Math.PI * 2);
        ctx.stroke();
      }
      if ((labels || c.isPlayer || selected) && e) {
        ctx.font = `${c.isPlayer ? 600 : 500} 10px ui-monospace, monospace`;
        ctx.fillStyle = c.isPlayer ? '#fde047' : '#cbd5e1';
        ctx.textAlign = 'left';
        ctx.fillText(`${e.position} ${e.driverLabel}`, tx(x) + r + 4, tz(z) + 3);
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

  // Click a car to select it (inspector only).
  function onClick(e: MouseEvent) {
    if (!interactive) return;
    const s = pitwall.snap;
    if (!s) return;
    const rect = canvas.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    // Re-derive transform from the last draw.
    const ref = pitwall.outline && pitwall.outline.length > 10 ? pitwall.outline : cloud;
    if (ref.length < 3) return;
    const b = bounds(ref);
    const w = rect.width,
      h = rect.height;
    const scale = Math.min((w - 36) / (b.x1 - b.x0 || 1), (h - 36) / (b.z1 - b.z0 || 1)) * zoom;
    const cx = (b.x0 + b.x1) / 2,
      cz = (b.z0 + b.z1) / 2;
    let best: { d: number; i: number } | null = null;
    for (const c of s.track.cars) {
      const d = Math.hypot(w / 2 + (c.x - cx) * scale + panX - mx, h / 2 + (c.z - cz) * scale + panY - my);
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
    min-height: 140px;
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
