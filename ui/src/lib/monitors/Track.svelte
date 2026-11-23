<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import TrackCanvas, { type ColorMode } from '$lib/components/TrackCanvas.svelte';
  import TraceCanvas from '$lib/components/TraceCanvas.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { lapTime, signed } from '$lib/format';
  import RadioComposer from '$lib/components/RadioComposer.svelte';
  import type { TelemetrySnapshot } from '$lib/types';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  let colorMode = $state<ColorMode>('positions');
  const selectedIdx = $derived(pitwall.selectedCar ?? snap.timing.playerIndex);
  const car = $derived(snap.track.cars.find((c) => c.carIndex === selectedIdx));
  const entry = $derived(snap.timing.entries.find((e) => e.carIndex === selectedIdx));
  const flagged = $derived(
    snap.session.marshalZones.map((z, i) => ({ ...z, zone: i + 1 })).filter((z) => z.flag === 'yellow' || z.flag === 'red' || z.flag === 'blue')
  );
  const sectors = $derived.by(() => {
    const zones = snap.session.marshalZones;
    return flagged.map((z) => {
      const s = z.startFraction < 1 / 3 ? 1 : z.startFraction < 2 / 3 ? 2 : 3;
      return { ...z, sector: s, endFraction: zones[z.zone]?.startFraction ?? 1 };
    });
  });
  const yellowSectors = $derived([...new Set(sectors.filter((s) => s.flag === 'yellow').map((s) => s.sector))]);
  const me = $derived(snap.track.cars.find((c) => c.isPlayer));
  const gearLabel = (g: number) => (g < 0 ? 'R' : g === 0 ? 'N' : String(g));
  const delta = $derived(snap.timing.lastLapMs && snap.timing.bestLapMs ? (snap.timing.lastLapMs - snap.timing.bestLapMs) / 1000 : null);
</script>

{#snippet legend()}
  <div class="legend">
    <span style="color:var(--data)">THR</span><span style="color:var(--crit)">BRK</span><span style="color:var(--accent)">SPD</span>
  </div>
{/snippet}

{#if mode === 'tile'}
  <div class="tile">
    <div class="map-area">
      <TrackCanvas />
      <div class="ro tl num">
        <div><span class="k">LAP</span> <b>{snap.session.lap}</b><span class="faint">/{snap.session.totalLaps}</span></div>
        <div><span class="k">LAST</span> {lapTime(snap.timing.lastLapMs)}</div>
        <div><span class="k">BEST</span> <span class="purple">{lapTime(snap.timing.bestLapMs)}</span></div>
        {#if delta !== null}<div><span class="k">DELTA</span> <span class:neg={delta <= 0} class:pos={delta > 0}>{signed(delta, 3)}</span></div>{/if}
      </div>
      {#if me}
        <div class="ro tr num">
          <div class="speed">{me.speedKph}<small> KPH</small></div>
          <div class="gear">{gearLabel(me.gear)}{#if me.drsActive}<span class="drs">DRS</span>{/if}</div>
        </div>
      {/if}
      {#if yellowSectors.length}
        <div class="flags"><span class="chip yellow"><Icon name="flag" size={11} /> YELLOW S{yellowSectors.join(', S')}</span></div>
      {/if}
    </div>
    <div class="trace">
      {@render legend()}
      <TraceCanvas />
    </div>
  </div>
{:else}
  <div class="inspect">
    <div class="toolbar">
      <span class="label">Colour</span>
      {#each [['positions', 'Positions'], ['speed', 'Speed'], ['inputs', 'Throttle / Brake'], ['ers', 'ERS deploy']] as [m, l]}
        <button class:active={colorMode === m} aria-pressed={colorMode === m} onclick={() => (colorMode = m as ColorMode)}>{l}</button>
      {/each}
      <span class="faint">Scroll to zoom · drag to pan · click a car to select</span>
    </div>
    <div class="map"><TrackCanvas interactive {colorMode} labels /></div>
    <div class="trace big">
      {@render legend()}
      <TraceCanvas />
    </div>
    <div class="cols">
      <div class="panel">
        <div class="label">Selected car</div>
        {#if car && entry}
          <div class="car-head">
            <strong>P{entry.position} {entry.driverLabel}</strong>
            <span class="dim">{entry.driverName}{entry.isPlayer ? ' · you' : ''}</span>
          </div>
          <div class="grid num">
            <span class="dim">Speed</span><span>{car.speedKph} km/h</span>
            <span class="dim">Gear</span><span>{car.gear <= 0 ? (car.gear === 0 ? 'N' : 'R') : car.gear}</span>
            <span class="dim">Throttle</span><span class="bar"><span style="width:{car.throttlePct}%;background:#22c55e"></span></span>
            <span class="dim">Brake</span><span class="bar"><span style="width:{car.brakePct}%;background:#ef4444"></span></span>
            <span class="dim">ERS mode</span><span>{#if car.ersDeployMode}{car.ersDeployMode}{:else}<Icon name="lock" size={12} /> restricted{/if}</span>
            <span class="dim">Lap distance</span><span>{car.lapDistancePct.toFixed(1)}%</span>
            <span class="dim">DRS</span><span>{car.drsActive ? 'OPEN' : '—'}</span>
          </div>
        {:else}
          <p class="faint">No car selected.</p>
        {/if}
      </div>
      <div class="panel">
        <div class="label">Flags by marshal zone</div>
        {#if sectors.length === 0}
          <p class="dim">All zones clear.</p>
        {:else}
          <ul>
            {#each sectors as z}
              <li><span class="chip {z.flag}"><Icon name="flag" size={11} /> {z.flag.toUpperCase()}</span> Zone {z.zone} · S{z.sector} · {(z.startFraction * 100).toFixed(0)}–{(z.endFraction * 100).toFixed(0)}% of lap</li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
    <RadioComposer
      presets={[
        yellowSectors.length ? `Yellow flag in sector ${yellowSectors[0]}, careful` : 'Traffic ahead, watch the blue flags',
        'Car behind on a fast lap, let them by',
        'Track is clear, push'
      ]}
      context={entry ? [`${entry.driverLabel} P${entry.position}`, car ? `${car.speedKph} km/h` : ''].filter(Boolean) : []}
    />
  </div>
{/if}

<style>
  .tile {
    height: 100%;
    min-height: 200px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .map-area {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .ro {
    position: absolute;
    top: 0;
    font-size: 11px;
    line-height: 1.45;
    pointer-events: none;
    text-shadow: 0 0 4px #000;
  }
  .ro .k {
    display: inline-block;
    width: 40px;
    font-family: var(--cond);
    letter-spacing: 0.12em;
    color: var(--text-dim);
  }
  .tl {
    left: 0;
    color: var(--data);
  }
  .tl b {
    font-size: 15px;
  }
  .tr {
    right: 0;
    text-align: right;
  }
  .speed {
    font-size: 24px;
    font-weight: 700;
    color: var(--text);
    line-height: 1;
  }
  .speed small {
    font-size: 9px;
    color: var(--text-dim);
  }
  .gear {
    font-size: 30px;
    font-weight: 700;
    color: var(--data);
    line-height: 1.1;
  }
  .drs {
    font-size: 10px;
    margin-left: 6px;
    padding: 1px 4px;
    color: #001a12;
    background: var(--data);
    vertical-align: middle;
  }
  .purple {
    color: var(--purple);
  }
  .neg {
    color: var(--data);
  }
  .pos {
    color: var(--crit);
  }
  .flags {
    position: absolute;
    left: 0;
    bottom: 0;
  }
  .trace {
    position: relative;
    height: 26%;
    min-height: 56px;
    max-height: 120px;
    border-top: 1px solid var(--hairline);
    padding-top: 4px;
  }
  .trace.big {
    height: 150px;
    max-height: none;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    padding: 6px;
  }
  .legend {
    position: absolute;
    left: 36px;
    top: 4px;
    z-index: 1;
    display: flex;
    gap: 8px;
    font-family: var(--cond);
    font-size: 9px;
    letter-spacing: 0.14em;
  }
  .chip.yellow {
    color: #facc15;
    border-color: #facc15;
  }
  .chip.red {
    color: var(--crit);
    border-color: var(--crit);
  }
  .chip.blue {
    color: var(--cold);
    border-color: var(--cold);
  }
  .inspect {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .toolbar .active {
    border-color: var(--accent);
    color: var(--accent);
    background: rgba(34, 211, 238, 0.12);
  }
  .map {
    height: min(56vh, 620px);
    min-height: 320px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: rgba(2, 7, 10, 0.6);
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 12px;
  }
  .car-head {
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin: 6px 0;
  }
  .grid {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 6px 10px;
    align-items: center;
  }
  ul {
    margin: 6px 0 0;
    padding-left: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
  }
</style>
