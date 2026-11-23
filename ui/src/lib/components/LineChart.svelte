<script lang="ts" module>
  export interface Series {
    label: string;
    color: string;
    points: [number, number][];
    dashed?: boolean;
  }
</script>

<script lang="ts">
  // Minimal dependency-free SVG line chart.

  let {
    series,
    height = 160,
    yLabel = '',
    xLabel = '',
    yMin = undefined,
    yMax = undefined,
    xFormat = (v: number) => String(Math.round(v)),
    yFormat = (v: number) => String(Math.round(v)),
    hLines = []
  }: {
    series: Series[];
    height?: number;
    yLabel?: string;
    xLabel?: string;
    yMin?: number;
    yMax?: number;
    xFormat?: (v: number) => string;
    yFormat?: (v: number) => string;
    hLines?: { y: number; color: string; label?: string }[];
  } = $props();

  const W = 600;
  const pad = { l: 44, r: 10, t: 10, b: 22 };

  const all = $derived(series.flatMap((s) => s.points));
  const bounds = $derived.by(() => {
    if (all.length === 0) return { x0: 0, x1: 1, y0: 0, y1: 1 };
    let x0 = Infinity,
      x1 = -Infinity,
      y0 = Infinity,
      y1 = -Infinity;
    for (const [x, y] of all) {
      x0 = Math.min(x0, x);
      x1 = Math.max(x1, x);
      y0 = Math.min(y0, y);
      y1 = Math.max(y1, y);
    }
    for (const h of hLines) {
      y0 = Math.min(y0, h.y);
      y1 = Math.max(y1, h.y);
    }
    if (yMin !== undefined) y0 = yMin;
    if (yMax !== undefined) y1 = Math.max(yMax, y1);
    if (x1 === x0) x1 = x0 + 1;
    if (y1 === y0) y1 = y0 + 1;
    const padY = (y1 - y0) * 0.08;
    return { x0, x1, y0: yMin !== undefined ? y0 : y0 - padY, y1: y1 + padY };
  });

  const sx = (x: number) => pad.l + ((x - bounds.x0) / (bounds.x1 - bounds.x0)) * (W - pad.l - pad.r);
  const sy = (y: number) => pad.t + (1 - (y - bounds.y0) / (bounds.y1 - bounds.y0)) * (height - pad.t - pad.b);
  const path = (pts: [number, number][]) => pts.map(([x, y], i) => `${i ? 'L' : 'M'}${sx(x).toFixed(1)},${sy(y).toFixed(1)}`).join('');
  const yTicks = $derived([0, 0.25, 0.5, 0.75, 1].map((f) => bounds.y0 + f * (bounds.y1 - bounds.y0)));
  const xTicks = $derived([0, 0.5, 1].map((f) => bounds.x0 + f * (bounds.x1 - bounds.x0)));
</script>

<figure>
  {#if all.length === 0}
    <div class="empty" style="height:{height}px">No data yet</div>
  {:else}
    <svg viewBox="0 0 {W} {height}" preserveAspectRatio="none" role="img" aria-label={yLabel}>
      {#each yTicks as t}
        <line x1={pad.l} x2={W - pad.r} y1={sy(t)} y2={sy(t)} class="grid" />
        <text x={pad.l - 6} y={sy(t) + 3} text-anchor="end" class="tick">{yFormat(t)}</text>
      {/each}
      {#each xTicks as t}
        <text x={sx(t)} y={height - 6} text-anchor="middle" class="tick">{xFormat(t)}</text>
      {/each}
      {#each hLines as h}
        <line x1={pad.l} x2={W - pad.r} y1={sy(h.y)} y2={sy(h.y)} style="stroke:{h.color}" stroke-dasharray="4 4" stroke-width="1" />
        {#if h.label}<text x={W - pad.r - 4} y={sy(h.y) - 4} text-anchor="end" class="tick" style="fill:{h.color}">{h.label}</text>{/if}
      {/each}
      {#each series as s}
        <path d={path(s.points)} style="stroke:{s.color};filter:drop-shadow(0 0 2px {s.color})" fill="none" stroke-width="1.8" stroke-dasharray={s.dashed ? '5 4' : undefined} vector-effect="non-scaling-stroke" />
      {/each}
    </svg>
    <figcaption>
      {#each series as s}<span><i style="background:{s.color}"></i>{s.label}</span>{/each}
      {#if xLabel}<span class="faint">x: {xLabel}</span>{/if}
    </figcaption>
  {/if}
</figure>

<style>
  figure {
    margin: 0;
  }
  svg {
    width: 100%;
    display: block;
  }
  .grid {
    stroke: var(--hairline);
  }
  .tick {
    font-family: var(--mono);
    font-size: 10px;
    fill: var(--text-faint);
  }
  figcaption {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    font-size: 11px;
    color: var(--text-dim);
    margin-top: 4px;
  }
  figcaption i {
    display: inline-block;
    width: 10px;
    height: 3px;
    border-radius: 2px;
    margin-right: 5px;
    vertical-align: middle;
  }
  .empty {
    display: grid;
    place-items: center;
    color: var(--text-faint);
    border: 1px dashed var(--hairline);
    border-radius: 2px;
    font-size: 12px;
  }
</style>
