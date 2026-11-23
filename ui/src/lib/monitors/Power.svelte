<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { signed } from '$lib/format';
  import type { TelemetrySnapshot } from '$lib/types';
  import LineChart from '$lib/components/LineChart.svelte';
  import RadioComposer from '$lib/components/RadioComposer.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const p = $derived(snap.powertrain);
  const fuelLevel = $derived(p.fuelRemainingLapsDelta <= -0.6 ? 'crit' : p.fuelRemainingLapsDelta <= -0.2 ? 'watch' : 'ok');
  const lapsLeft = $derived(Math.max(0, snap.session.totalLaps - snap.session.lap + 1));
  // Kg to save per remaining lap to finish (if short on fuel).
  const saveKgPerLap = $derived(
    p.fuelRemainingLapsDelta < 0 && p.fuelBurnPerLapKg && lapsLeft > 0 ? (-p.fuelRemainingLapsDelta * p.fuelBurnPerLapKg) / lapsLeft : 0
  );
  const components = $derived([
    ['ICE', p.wearPct.ice],
    ['MGU-H', p.wearPct.mguH],
    ['MGU-K', p.wearPct.mguK],
    ['ES', p.wearPct.es],
    ['CE', p.wearPct.ce],
    ['TC (turbo)', p.wearPct.tc],
    ['Gearbox', p.wearPct.gearbox]
  ] as [string, number][]);
  const health = $derived(
    p.engineBlown || p.engineSeized ? 'ENGINE FAILURE' : p.ersFault ? 'ERS FAULT' : components.some(([, v]) => v >= 70) ? 'WEAR HIGH' : 'HEALTHY'
  );
  const mine = $derived(pitwall.laps.filter((l) => l.carIndex === snap.timing.playerIndex));
  const fuelSeries = $derived.by(() => {
    const burns: [number, number][] = [];
    for (let i = 1; i < mine.length; i++) {
      const a = mine[i - 1].fuelKg,
        b = mine[i].fuelKg;
      if (a != null && b != null && a > b) burns.push([mine[i].lap, a - b]);
    }
    return [{ label: 'Burn per lap (kg)', color: 'var(--accent)', points: burns }];
  });
  const ersSeries = $derived([
    { label: 'Harvested (MJ)', color: '#22c55e', points: mine.filter((l) => l.ersHarvestJ != null).map((l) => [l.lap, l.ersHarvestJ! / 1e6] as [number, number]) },
    { label: 'Deployed (MJ)', color: '#f472b6', points: mine.filter((l) => l.ersDeployJ != null).map((l) => [l.lap, l.ersDeployJ! / 1e6] as [number, number]) }
  ]);
</script>

{#if mode === 'tile'}
  <div class="tile">
    <div class="fuel {fuelLevel}">
      <div class="label">Fuel vs target</div>
      <div class="big num">{signed(p.fuelRemainingLapsDelta, 2)} <small>laps</small></div>
      <div class="dim num">{p.fuelRemainingKg.toFixed(1)} kg{p.fuelBurnPerLapKg ? ` · ${p.fuelBurnPerLapKg.toFixed(2)} kg/lap` : ''}</div>
    </div>
    <div>
      <div class="row"><span class="label">ERS</span><span class="num">{p.ersStorePct.toFixed(0)}%</span><span class="chip">{p.ersDeployMode}</span></div>
      <div class="bar ers"><span style="width:{p.ersStorePct}%;background:{p.ersStorePct < 10 ? 'var(--watch)' : '#38bdf8'}"></span></div>
    </div>
    <div class="row"><span class="label">Power unit</span><span class="health" class:bad={health !== 'HEALTHY'}>{health}</span></div>
  </div>
{:else}
  <div class="inspect">
    <div class="kpis">
      <div class="kpi {fuelLevel}"><div class="label">Fuel delta (MFD)</div><div class="big num">{signed(p.fuelRemainingLapsDelta, 2)} laps</div></div>
      <div class="kpi"><div class="label">Fuel in tank</div><div class="big num">{p.fuelRemainingKg.toFixed(2)} kg</div></div>
      <div class="kpi"><div class="label">Burn / lap (avg 3)</div><div class="big num">{p.fuelBurnPerLapKg?.toFixed(3) ?? '—'} kg</div></div>
      <div class="kpi"><div class="label">Required / lap to finish</div><div class="big num">{p.fuelRequiredPerLapKg?.toFixed(3) ?? '—'} kg</div></div>
      <div class="kpi" class:watch={saveKgPerLap > 0}>
        <div class="label">Lift &amp; coast target</div>
        <div class="big num">{saveKgPerLap > 0 ? `save ${saveKgPerLap.toFixed(3)} kg/lap` : 'not needed'}</div>
      </div>
      <div class="kpi"><div class="label">ERS store</div><div class="big num">{p.ersStorePct.toFixed(1)}% · {p.ersDeployMode}</div></div>
      <div class="kpi"><div class="label">ERS this lap</div><div class="big num">+{p.ersHarvestedThisLapKJ.toFixed(0)} / −{p.ersDeployedThisLapKJ.toFixed(0)} kJ</div></div>
    </div>
    <div class="charts">
      <div><div class="label">Fuel burn per lap</div><LineChart series={fuelSeries} xLabel="lap" yFormat={(v) => v.toFixed(2)} hLines={p.fuelRequiredPerLapKg ? [{ y: p.fuelRequiredPerLapKg, color: 'var(--watch)', label: 'required' }] : []} /></div>
      <div><div class="label">ERS per lap</div><LineChart series={ersSeries} xLabel="lap" yFormat={(v) => v.toFixed(1)} /></div>
    </div>
    <div class="panel">
      <div class="label">Component wear</div>
      <div class="comp">
        {#each components as [name, v]}
          <span>{name}</span>
          <span class="bar"><span style="width:{v}%;background:{v >= 70 ? 'var(--crit)' : v >= 50 ? 'var(--watch)' : 'var(--ok)'}"></span></span>
          <span class="num">{v}%</span>
        {/each}
      </div>
      <div class="flags">
        {#if p.ersFault}<span class="chip crit">ERS FAULT</span>{/if}
        {#if p.engineBlown}<span class="chip crit">ENGINE BLOWN</span>{/if}
        {#if p.engineSeized}<span class="chip crit">ENGINE SEIZED</span>{/if}
      </div>
    </div>
    <RadioComposer
      presets={['Lift and coast for fuel, 100 metres before the big stops', 'Fuel is fine, full push', 'Deploy ERS, overtake mode now', 'Harvest this lap, battery is low']}
      context={[`Fuel ${signed(p.fuelRemainingLapsDelta, 1)} laps`, `ERS ${p.ersStorePct.toFixed(0)}%`]}
    />
  </div>
{/if}

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .big {
    font-size: 24px;
    font-weight: 600;
  }
  .big small {
    font-size: 12px;
    color: var(--text-dim);
  }
  .fuel.ok .big {
    color: var(--ok);
  }
  .fuel.watch .big,
  .kpi.watch .big {
    color: var(--watch);
  }
  .fuel.crit .big,
  .kpi.crit .big {
    color: var(--crit);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }
  .row .label {
    flex: 1;
  }
  .ers {
    height: 10px;
  }
  .health {
    font-size: 12px;
    font-weight: 600;
    color: var(--ok);
  }
  .health.bad {
    color: var(--crit);
  }
  .inspect {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .kpis {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 10px;
  }
  .kpi {
    padding: 10px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .kpi .big {
    font-size: 17px;
    margin-top: 4px;
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 14px;
  }
  .panel {
    padding: 10px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
  }
  .comp {
    display: grid;
    grid-template-columns: 90px 1fr 44px;
    gap: 6px 10px;
    align-items: center;
    margin-top: 8px;
    font-size: 13px;
  }
  .comp .bar {
    height: 8px;
  }
  .flags {
    display: flex;
    gap: 6px;
    margin-top: 8px;
  }
  .chip.crit {
    color: #fff;
    background: var(--crit);
    border-color: var(--crit);
  }
</style>
