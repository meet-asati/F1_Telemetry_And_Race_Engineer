<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { clock, timeOfDay, weatherIcon, weatherLabel } from '$lib/format';
  import type { TelemetrySnapshot } from '$lib/types';
  import RadioComposer from '$lib/components/RadioComposer.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const s = $derived(snap.session);
  const next = $derived(s.weatherForecast.filter((f) => f.timeOffsetMin > 0));
  const rainSoon = $derived(next.find((f) => f.rainPct >= 50 && f.timeOffsetMin <= 30));
  const sc = $derived(
    ({ full: 'SAFETY CAR', virtual: 'VIRTUAL SAFETY CAR', formation: 'FORMATION LAP', none: '' } as Record<string, string>)[s.safetyCarStatus] ?? ''
  );
  const remaining = $derived(s.isRace ? `${Math.max(0, s.totalLaps - s.lap)} laps to go` : `${clock(s.timeLeftMs)} left`);
</script>

{#if mode === 'tile'}
  <div class="tile">
    <div class="now">
      <span class="icon" aria-hidden="true">{weatherIcon(s.weather)}</span>
      <div>
        <div class="cond">{weatherLabel(s.weather)}</div>
        <div class="dim num">Air {s.airTempC}° · Track {s.trackTempC}°</div>
      </div>
    </div>
    <div class="forecast num">
      {#each next.slice(0, 5) as f}
        <div class="fc" title="{f.timeOffsetMin} min: {weatherLabel(f.weather)}, rain {f.rainPct}%">
          <span>{weatherIcon(f.weather)}</span>
          <span class="rain" style="--r:{f.rainPct}%">{f.rainPct}%</span>
          <span class="faint">+{f.timeOffsetMin}′</span>
        </div>
      {/each}
    </div>
    <div class="row">
      <span class="dim num">{remaining}</span>
      {#if sc}<span class="sc">{sc}</span>{:else if rainSoon}<span class="chip warn">Rain {rainSoon.rainPct}% in {rainSoon.timeOffsetMin}′</span>{/if}
    </div>
  </div>
{:else}
  <div class="inspect">
    <div class="row wrap">
      <span class="chip">{s.trackName} · {s.sessionType}</span>
      <span class="chip num">{remaining}</span>
      <span class="chip">Forecast accuracy: {s.forecastAccuracy}</span>
      {#if sc}<span class="sc">{sc}</span>{/if}
    </div>
    <div class="label">Forecast timeline (rain probability)</div>
    <div class="timeline num">
      {#each s.weatherForecast as f}
        <div class="slot">
          <div class="barwrap"><div class="fill" style="height:{f.rainPct}%"></div></div>
          <div class="pct">{f.rainPct}%</div>
          <div>{weatherIcon(f.weather)}</div>
          <div class="faint">{f.timeOffsetMin === 0 ? 'now' : `+${f.timeOffsetMin}′`}</div>
          <div class="faint small">{f.trackTempC}° / {f.airTempC}°</div>
        </div>
      {/each}
      {#if s.weatherForecast.length === 0}<p class="faint">No forecast samples received.</p>{/if}
    </div>
    <div class="label">Session &amp; flag log</div>
    <ul class="log">
      {#each pitwall.raceLog as e}
        <li class={e.level}><span class="faint num">{timeOfDay(e.atMs)}</span> {e.text}</li>
      {:else}
        <li class="faint">No flags or penalties yet.</li>
      {/each}
    </ul>
    <RadioComposer
      presets={['Inters expected soon, stay out for now', 'Box for intermediates, box box', 'Rain is easing, slicks next stop', 'Safety car, box this lap']}
      context={[`${weatherLabel(s.weather)}`, `Track ${s.trackTempC}°C`, rainSoon ? `Rain ${rainSoon.rainPct}% in ${rainSoon.timeOffsetMin} min` : ''].filter(Boolean)}
    />
  </div>
{/if}

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .now {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .icon {
    font-size: 30px;
  }
  .cond {
    font-weight: 600;
  }
  .forecast {
    display: flex;
    gap: 6px;
  }
  .fc {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    font-size: 11px;
    padding: 4px 2px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
  }
  .rain {
    color: color-mix(in srgb, #60a5fa var(--r), var(--text-dim));
    font-weight: 600;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: space-between;
  }
  .row.wrap {
    flex-wrap: wrap;
    justify-content: flex-start;
  }
  .sc {
    background: #facc15;
    color: #111;
    font-weight: 800;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    letter-spacing: 0.06em;
  }
  .warn {
    color: #60a5fa;
    border-color: #60a5fa;
  }
  .inspect {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .timeline {
    display: flex;
    gap: 8px;
    overflow-x: auto;
  }
  .slot {
    min-width: 64px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    font-size: 12px;
  }
  .barwrap {
    width: 26px;
    height: 90px;
    background: var(--surface);
    border-radius: 4px;
    display: flex;
    align-items: flex-end;
    overflow: hidden;
  }
  .fill {
    width: 100%;
    background: linear-gradient(180deg, #3b82f6, #1d4ed8);
  }
  .pct {
    font-weight: 600;
  }
  .small {
    font-size: 10.5px;
  }
  .log {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 200px;
    overflow: auto;
    font-size: 13px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .log .critical {
    color: var(--crit);
  }
  .log .watch {
    color: var(--watch);
  }
</style>
