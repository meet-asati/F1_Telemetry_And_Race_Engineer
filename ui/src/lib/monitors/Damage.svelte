<script lang="ts">
  import { CORNERS, tempColor } from '$lib/format';
  import type { TelemetrySnapshot } from '$lib/types';
  import RadioComposer from '$lib/components/RadioComposer.svelte';
  import CarWireframe from '$lib/components/CarWireframe.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const d = $derived(snap.damage);
  const brakes = $derived(snap.tyres.brakeTempC);
  const peakBrake = $derived(Math.max(...brakes));
  const dmgColor = (v: number) => (v >= 30 ? 'var(--crit)' : v >= 10 ? 'var(--watch)' : v > 0 ? 'var(--warm)' : 'var(--data)');
  const tintOf = (v: number) => (v > 0 ? dmgColor(v) : null);
  const parts = $derived([
    ['FL wing', d.frontLeftWingPct],
    ['FR wing', d.frontRightWingPct],
    ['Rear wing', d.rearWingPct],
    ['Floor', d.floorPct],
    ['Diffuser', d.diffuserPct],
    ['Sidepods', d.sidepodPct]
  ] as [string, number][]);
</script>

{#snippet carDiagram(size: number)}
  <CarWireframe
    height={size}
    label="Chassis damage and brake temperatures"
    parts={{
      frontLeftWing: tintOf(d.frontLeftWingPct),
      frontRightWing: tintOf(d.frontRightWingPct),
      rearWing: tintOf(d.rearWingPct),
      floor: tintOf(d.floorPct),
      diffuser: tintOf(d.diffuserPct),
      sidepods: tintOf(d.sidepodPct)
    }}
    wheels={brakes.map((b) => tempColor(b, 300, 1000))}
  />
{/snippet}

{#if mode === 'tile'}
  <div class="tile">
    {@render carDiagram(150)}
    <div class="list num">
      {#each parts.slice(0, 5) as [n, v]}
        <span class="dim">{n}</span><span style="color:{dmgColor(v)}">{v}%</span>
      {/each}
      <span class="dim">Brakes peak</span><span style="color:{tempColor(peakBrake, 300, 1000)}">{peakBrake}°C</span>
      {#if d.drsFault}<span class="crit" style="grid-column: span 2">DRS FAULT</span>{/if}
    </div>
  </div>
{:else}
  <div class="inspect">
    <div class="cols">
      <div class="diagram">{@render carDiagram(320)}</div>
      <div class="panel">
        <div class="label">Aero &amp; chassis</div>
        <div class="list big num">
          {#each parts as [n, v]}
            <span>{n}</span>
            <span class="bar"><span style="width:{v}%;background:{dmgColor(v)}"></span></span>
            <span>{v}%</span>
          {/each}
        </div>
        <div class="label" style="margin-top:12px">Brakes</div>
        <div class="brakes num">
          {#each [0, 1, 2, 3] as i}
            <div>
              <span class="dim">{CORNERS[i]}</span>
              <strong style="color:{tempColor(brakes[i], 300, 1000)}">{brakes[i]}°C</strong>
              <span class="faint">wear {d.brakeDamagePct[i]}%</span>
            </div>
          {/each}
        </div>
        {#if d.drsFault}<p class="crit">DRS fault reported</p>{/if}
      </div>
      <div class="panel">
        <div class="label">Car setup</div>
        {#if d.setup}
          <div class="setup num">
            <span class="dim">Wings F / R</span><span>{d.setup.frontWing} / {d.setup.rearWing}</span>
            <span class="dim">Diff on / off</span><span>{d.setup.onThrottle}% / {d.setup.offThrottle}%</span>
            <span class="dim">Brake bias</span><span>{d.setup.brakeBias}%</span>
            <span class="dim">Brake pressure</span><span>{d.setup.brakePressure}%</span>
            <span class="dim">Suspension F / R</span><span>{d.setup.frontSuspension} / {d.setup.rearSuspension}</span>
            <span class="dim">Anti-roll F / R</span><span>{d.setup.frontAntiRollBar} / {d.setup.rearAntiRollBar}</span>
            <span class="dim">Ride height F / R</span><span>{d.setup.frontRideHeight} / {d.setup.rearRideHeight}</span>
          </div>
        {:else}
          <p class="faint">Setup not received yet (or restricted).</p>
        {/if}
      </div>
    </div>
    <RadioComposer
      presets={['Box for a new front wing', 'Wing damage is minor, stay out', 'Watch the brake temps, lift earlier', 'Move the brake bias rearward two clicks']}
      context={parts.filter(([, v]) => v > 0).map(([n, v]) => `${n} ${v}%`).concat(`Brakes peak ${peakBrake}°C`)}
    />
  </div>
{/if}

<style>
  .tile {
    display: flex;
    gap: 12px;
    align-items: center;
    height: 100%;
  }
  .tile > :global(svg) {
    height: 100%;
    width: auto;
    max-height: 190px;
  }
  .list {
    display: grid;
    grid-template-columns: auto auto;
    gap: 3px 10px;
    font-size: 12.5px;
  }
  .list.big {
    grid-template-columns: 90px 1fr 44px;
    align-items: center;
    gap: 6px 10px;
    margin-top: 8px;
  }
  .list .bar {
    height: 8px;
  }
  .crit {
    color: var(--crit);
    font-weight: 600;
  }
  .inspect {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .cols {
    display: grid;
    grid-template-columns: auto 1fr 1fr;
    gap: 14px;
    align-items: start;
  }
  @media (max-width: 900px) {
    .cols {
      grid-template-columns: 1fr;
    }
  }
  .diagram {
    display: grid;
    place-items: center;
  }
  .brakes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-top: 6px;
  }
  .brakes div {
    display: flex;
    gap: 6px;
    align-items: baseline;
  }
  .setup {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px;
    margin-top: 8px;
  }
</style>
