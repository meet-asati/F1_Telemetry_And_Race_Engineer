<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { COMPOUND_COLOR, CORNERS, gap, lapTime, signed, wearColor } from '$lib/format';

  const s = $derived(pitwall.snap);
  const worst = $derived.by(() => {
    if (!s) return null;
    const w = s.tyres.wearPct;
    const i = w.indexOf(Math.max(...w));
    return { corner: CORNERS[i], value: w[i] };
  });
  const flag = $derived(
    s?.session.safetyCarStatus === 'full' ? 'SAFETY CAR' : s?.session.safetyCarStatus === 'virtual' ? 'VSC' : s?.session.marshalZones.some((z) => z.flag === 'red') ? 'RED FLAG' : null
  );
  const lastVsBest = $derived(s?.timing.lastLapMs && s.timing.bestLapMs ? (s.timing.lastLapMs - s.timing.bestLapMs) / 1000 : null);
</script>

<svelte:head><title>Driver · Pit Wall</title></svelte:head>

<div class="driver" class:stale={pitwall.stale}>
  {#if flag}<div class="flag">{flag}</div>{/if}
  {#if s && s.timing.position}
    <div class="grid">
      <div class="cell pos"><div class="k">POS</div><div class="v num">P{s.timing.position}</div><div class="sub num">Lap {s.session.lap}/{s.session.totalLaps}</div></div>
      <div class="cell"><div class="k">AHEAD</div><div class="v num">{gap(s.timing.gapAheadMs, '−')}</div></div>
      <div class="cell"><div class="k">BEHIND</div><div class="v num">{gap(s.timing.gapBehindMs)}</div></div>
      <div class="cell">
        <div class="k">LAST LAP</div>
        <div class="v num small">{lapTime(s.timing.lastLapMs)}</div>
        <div class="sub num" class:good={lastVsBest !== null && lastVsBest <= 0}>{lastVsBest !== null ? signed(lastVsBest, 3, 's') : ''}</div>
      </div>
      <div class="cell" class:warn={s.powertrain.fuelRemainingLapsDelta <= -0.2}>
        <div class="k">FUEL</div><div class="v num">{signed(s.powertrain.fuelRemainingLapsDelta, 1)}</div><div class="sub">laps</div>
      </div>
      <div class="cell"><div class="k">ERS</div><div class="v num">{s.powertrain.ersStorePct.toFixed(0)}%</div><div class="sub">{s.powertrain.ersDeployMode}</div></div>
      <div class="cell">
        <div class="k">TYRES</div>
        <div class="v num" style="color:{worst ? wearColor(worst.value) : 'inherit'}">{worst?.value.toFixed(0)}%</div>
        <div class="sub"><span class="dot" style="color:{COMPOUND_COLOR[s.tyres.compound]}"></span> {s.tyres.compound} · {worst?.corner} · {s.tyres.ageLaps}L</div>
      </div>
    </div>
    <div class="radio">
      <span class="k">RADIO</span>
      <span class="line">{s.comms.lastSpokenLine ?? '—'}</span>
      <button class="ghost" onclick={() => pitwall.send({ type: 'comms.mute', muted: !s.comms.muted })}><Icon name={s.comms.muted ? 'mute' : 'volume'} size={20} />{s.comms.muted ? 'Unmute AI' : 'Mute AI'}</button>
    </div>
  {:else}
    <div class="wait">{pitwall.connected ? 'Waiting for telemetry…' : 'Connecting…'}</div>
  {/if}
</div>

<style>
  .driver {
    height: 100vh;
    display: flex;
    flex-direction: column;
    padding: 12px;
    gap: 12px;
    background: #000;
  }
  .driver.stale {
    opacity: 0.5;
  }
  .flag {
    background: #facc15;
    color: #000;
    text-align: center;
    font-size: 40px;
    font-weight: 900;
    letter-spacing: 0.1em;
    border-radius: 8px;
  }
  .grid {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    grid-auto-rows: minmax(0, 1fr);
    gap: 12px;
  }
  .cell {
    background: var(--bezel);
    border: 1px solid var(--hairline);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    container-type: inline-size;
    overflow: hidden;
  }
  @media (max-width: 800px) {
    .grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
  .cell.warn {
    border-color: var(--watch);
    color: var(--watch);
  }
  .k {
    font-size: 14px;
    letter-spacing: 0.14em;
    color: var(--text-dim);
  }
  .v {
    font-size: clamp(28px, 24cqi, 96px);
    white-space: nowrap;
    font-weight: 700;
    line-height: 1.05;
  }
  .v.small {
    font-size: clamp(24px, 17cqi, 72px);
  }
  .pos .v {
    color: var(--player);
  }
  .sub {
    font-size: 18px;
    color: var(--text-dim);
  }
  .sub.good {
    color: #c084fc;
  }
  .radio {
    display: flex;
    gap: 14px;
    align-items: center;
    background: var(--bezel);
    border-radius: 12px;
    padding: 14px;
    font-size: 22px;
  }
  .line {
    flex: 1;
  }
  .wait {
    margin: auto;
    font-size: 24px;
    color: var(--text-dim);
  }
</style>
