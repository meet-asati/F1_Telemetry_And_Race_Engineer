<script lang="ts">
  import { CORNERS, tempColor } from '$lib/format';
  import type { TelemetrySnapshot } from '$lib/types';
  import RadioComposer from '$lib/components/RadioComposer.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const d = $derived(snap.damage);
  const brakes = $derived(snap.tyres.brakeTempC);
  const peakBrake = $derived(Math.max(...brakes));
  const dmgColor = (v: number) => (v >= 30 ? 'var(--crit)' : v >= 10 ? 'var(--watch)' : v > 0 ? '#a3e635' : '#334155');
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
  <svg viewBox="0 0 120 240" width={size * 0.5} height={size} role="img" aria-label="Chassis damage diagram">
    <!-- front wing -->
    <rect x="10" y="8" width="48" height="14" rx="3" fill={dmgColor(d.frontLeftWingPct)} />
    <rect x="62" y="8" width="48" height="14" rx="3" fill={dmgColor(d.frontRightWingPct)} />
    <!-- nose + chassis -->
    <path d="M52 22 L68 22 L72 70 L48 70 Z" fill="#1e293b" stroke="#475569" />
    <!-- sidepods -->
    <rect x="26" y="90" width="18" height="70" rx="6" fill={dmgColor(d.sidepodPct)} />
    <rect x="76" y="90" width="18" height="70" rx="6" fill={dmgColor(d.sidepodPct)} />
    <!-- floor -->
    <rect x="44" y="70" width="32" height="120" rx="6" fill={dmgColor(d.floorPct)} />
    <!-- diffuser -->
    <path d="M40 190 L80 190 L86 208 L34 208 Z" fill={dmgColor(d.diffuserPct)} />
    <!-- rear wing -->
    <rect x="18" y="214" width="84" height="16" rx="3" fill={dmgColor(d.rearWingPct)} />
    <!-- wheels with brake temps -->
    {#each [[6, 40], [96, 40], [6, 160], [96, 160]] as [x, y], i}
      <rect {x} {y} width="18" height="36" rx="4" fill="#0b0e14" style="stroke:{tempColor(brakes[i], 300, 1000)}" stroke-width="2.5" />
    {/each}
  </svg>
{/snippet}

{#if mode === 'tile'}
  <div class="tile">
    {@render carDiagram(150)}
    <div class="list num">
      {#each parts.slice(0, 5) as [n, v]}
        <span class="dim">{n}</span><span style="color:{v ? dmgColor(v) : 'var(--text-dim)'}">{v}%</span>
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
  .panel {
    padding: 10px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
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
