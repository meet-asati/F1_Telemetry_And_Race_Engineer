<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import TrackCanvas, { type ColorMode } from '$lib/components/TrackCanvas.svelte';
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
</script>

{#if mode === 'tile'}
  <div class="tile">
    <TrackCanvas />
    {#if yellowSectors.length}
      <div class="flags"><span class="chip yellow">⚑ Yellow S{yellowSectors.join(', S')}</span></div>
    {/if}
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
            <span class="dim">ERS mode</span><span>{car.ersDeployMode ?? '🔒'}</span>
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
              <li><span class="chip {z.flag}">⚑ {z.flag.toUpperCase()}</span> Zone {z.zone} · S{z.sector} · {(z.startFraction * 100).toFixed(0)}–{(z.endFraction * 100).toFixed(0)}% of lap</li>
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
    position: relative;
    height: 100%;
    min-height: 160px;
  }
  .flags {
    position: absolute;
    left: 0;
    bottom: 0;
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
  }
  .map {
    height: 420px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--canvas);
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 12px;
  }
  .panel {
    padding: 10px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
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
