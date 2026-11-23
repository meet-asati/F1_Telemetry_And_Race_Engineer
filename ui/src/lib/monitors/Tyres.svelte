<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { COMPOUND_COLOR, CORNERS, tempColor, wearColor } from '$lib/format';
  import type { TelemetrySnapshot } from '$lib/types';
  import LineChart from '$lib/components/LineChart.svelte';
  import RadioComposer from '$lib/components/RadioComposer.svelte';
  import CarWireframe from '$lib/components/CarWireframe.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const t = $derived(snap.tyres);
  const win = $derived(snap.session.pitWindow);
  const CORNER_COLORS = ['#38bdf8', '#a78bfa', '#f472b6', '#fb923c'];

  const hist = $derived(pitwall.tyreHist);
  const t0 = $derived(hist[0]?.t ?? 0);
  const thermal = $derived(
    [0, 1, 2, 3].flatMap((i) => [
      { label: `${CORNERS[i]} surface`, color: CORNER_COLORS[i], points: hist.map((s) => [(s.t - t0) / 60000, s.surface[i]] as [number, number]) },
      { label: `${CORNERS[i]} inner`, color: CORNER_COLORS[i], dashed: true, points: hist.map((s) => [(s.t - t0) / 60000, s.inner[i]] as [number, number]) }
    ])
  );
  const brakes = $derived(
    [0, 1, 2, 3].map((i) => ({ label: CORNERS[i], color: CORNER_COLORS[i], points: hist.map((s) => [(s.t - t0) / 60000, s.brake[i]] as [number, number]) }))
  );
  const wearByLap = $derived.by(() => {
    const mine = pitwall.laps.filter((l) => l.carIndex === snap.timing.playerIndex && l.wear);
    return [0, 1, 2, 3].map((i) => ({ label: CORNERS[i], color: CORNER_COLORS[i], points: mine.map((l) => [l.lap, l.wear![i]] as [number, number]) }));
  });
  const worst = $derived(Math.max(...t.wearPct));
  const wheelStroke = $derived(t.surfaceTempC.map((c) => tempColor(c)));
  const wheelFill = $derived(t.wearPct.map((w) => wearColor(w)));
</script>

{#snippet readout(i: number)}
  <div class="ro" class:right={i % 2 === 1}>
    <div class="cn">{CORNERS[i]}</div>
    <div class="w num" style="color:{wearColor(t.wearPct[i])}">{t.wearPct[i].toFixed(0)}<small>%</small></div>
    <div class="tt num">
      <span style="color:{tempColor(t.surfaceTempC[i])}" title="Surface">{t.surfaceTempC[i]}°</span><span class="faint">/</span><span
        style="color:{tempColor(t.innerTempC[i], 85, 115)}"
        title="Inner carcass">{t.innerTempC[i]}°</span
      >
    </div>
    <div class="faint num">{t.pressurePsi[i].toFixed(1)} psi</div>
  </div>
{/snippet}

{#snippet corner(i: number, big: boolean)}
  <div class="corner" class:big style="--wc:{wearColor(t.wearPct[i])}">
    <div class="cn label">{CORNERS[i]}</div>
    <div class="wear num">{t.wearPct[i].toFixed(0)}<small>%</small></div>
    <div class="temps num">
      <span style="color:{tempColor(t.surfaceTempC[i])}" title="Surface">{t.surfaceTempC[i]}°</span>
      <span class="faint">/</span>
      <span style="color:{tempColor(t.innerTempC[i], 85, 115)}" title="Inner carcass">{t.innerTempC[i]}°</span>
    </div>
    {#if big}
      <div class="extra num dim">
        {t.wearRatePerLap[i] != null ? `${t.wearRatePerLap[i]!.toFixed(2)} %/lap` : 'rate —'}
        · dmg {t.damagePct[i]}%{t.blistersPct ? ` · blister ${t.blistersPct[i]}%` : ''}
      </div>
      <div class="extra num dim">brake <span style="color:{tempColor(t.brakeTempC[i], 300, 1000)}">{t.brakeTempC[i]}°</span> · {t.pressurePsi[i].toFixed(1)} psi</div>
    {/if}
  </div>
{/snippet}

{#if mode === 'tile'}
  <div class="tile">
    <div class="head">
      <span class="compound" style="color:{COMPOUND_COLOR[t.compound] ?? '#888'}">{t.compound || '?'}</span>
      <span class="num">{t.actualCompound}</span>
      <span class="dim num">{t.ageLaps} L</span>
      {#if t.projectedPitLap}<span class="chip num box" title="Projected pit lap">BOX ~L{t.projectedPitLap}</span>{/if}
    </div>
    <div class="wire">
      <div class="side">{@render readout(0)}{@render readout(2)}</div>
      <div class="carbox"><CarWireframe wheels={wheelStroke} {wheelFill} label="Tyre wear and temperature by corner" /></div>
      <div class="side">{@render readout(1)}{@render readout(3)}</div>
    </div>
  </div>
{:else}
  <div class="inspect">
    <div class="summary">
      <span class="compound" style="color:{COMPOUND_COLOR[t.compound] ?? '#888'}">{t.compound}</span>
      <strong>{t.actualCompound}</strong>
      <span class="dim">{t.ageLaps} laps old</span>
      <span class="chip num">Projected pit lap: {t.projectedPitLap ?? '—'}</span>
      {#if win}<span class="chip num">Game pit window: L{win.idealLap}–L{win.latestLap} · rejoin P{win.rejoinPosition}</span>{/if}
    </div>
    <div class="big-row">
      <div class="carbox big"><CarWireframe wheels={wheelStroke} {wheelFill} height={300} label="Tyre wear and temperature by corner" /></div>
      <div class="car big-car">
        {#each [0, 1, 2, 3] as i}{@render corner(i, true)}{/each}
      </div>
    </div>
    <div class="charts">
      <div>
        <div class="label">Wear by lap (%)</div>
        <LineChart series={wearByLap} xLabel="lap" yMin={0} hLines={[{ y: 65, color: 'var(--crit)', label: 'critical' }, { y: 50, color: 'var(--watch)', label: 'watch' }]} />
      </div>
      <div>
        <div class="label">Surface (solid) vs inner (dashed) °C</div>
        <LineChart series={thermal} xLabel="minutes" xFormat={(v) => v.toFixed(1)} />
      </div>
      <div>
        <div class="label">Brake temperature °C</div>
        <LineChart series={brakes} xLabel="minutes" xFormat={(v) => v.toFixed(1)} hLines={[{ y: 1000, color: 'var(--watch)', label: 'hot' }]} />
      </div>
    </div>
    <RadioComposer
      presets={['Cool the front tyres on the straights', 'Look after the tyres, manage the slides', 'Box this lap for a new set', 'Tyres are good, push']}
      context={[...[0, 1, 2, 3].map((i) => `${CORNERS[i]} wear ${t.wearPct[i].toFixed(0)}%`), t.projectedPitLap ? `Projected pit L${t.projectedPitLap}` : ''].filter(Boolean)}
    />
    <p class="faint small">Worst corner {worst.toFixed(1)}%. Projection is a linear fit of this stint's wear per lap to {70}%.</p>
  </div>
{/if}

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 6px;
    height: 100%;
  }
  .box {
    color: var(--watch);
    border-color: var(--watch);
    margin-left: auto;
  }
  .wire {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    gap: 6px;
    align-items: stretch;
  }
  .side {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: 6% 0;
  }
  .carbox {
    min-height: 0;
    display: flex;
    justify-content: center;
  }
  .carbox :global(svg) {
    height: 100%;
    width: auto;
  }
  .carbox.big :global(svg) {
    height: 300px;
  }
  .ro {
    line-height: 1.15;
  }
  .ro.right {
    text-align: right;
  }
  .ro .cn {
    font-family: var(--cond);
    font-size: 10px;
    letter-spacing: 0.18em;
    color: var(--text-dim);
  }
  .ro .w {
    font-size: clamp(16px, 2.2vw, 26px);
    font-weight: 700;
    text-shadow: 0 0 8px currentColor;
  }
  .ro .w small {
    font-size: 10px;
    color: var(--text-dim);
    text-shadow: none;
  }
  .ro .tt,
  .ro .faint {
    font-size: 11px;
  }
  .big-row {
    display: flex;
    gap: 24px;
    align-items: center;
    flex-wrap: wrap;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .car {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    flex: 1;
  }
  .corner {
    border: 1px solid var(--hairline);
    border-left: 4px solid var(--wc);
    border-radius: 6px;
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: rgba(4, 16, 22, 0.6);
  }
  .wear {
    font-size: 22px;
    font-weight: 600;
    color: var(--wc);
    line-height: 1.1;
  }
  .wear small {
    font-size: 12px;
    color: var(--text-dim);
  }
  .temps {
    font-size: 12px;
  }
  .corner.big .wear {
    font-size: 30px;
  }
  .extra {
    font-size: 11.5px;
  }
  .inspect {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  .big-car {
    flex: 1;
    min-width: 300px;
    max-width: 640px;
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 14px;
  }
  .small {
    font-size: 12px;
    margin: 0;
  }
</style>
