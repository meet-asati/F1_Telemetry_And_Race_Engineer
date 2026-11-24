<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { COMPOUND_COLOR, gap, lapTime, sector, teamColor } from '$lib/format';
  import type { TelemetrySnapshot } from '$lib/types';
  import LineChart from '$lib/components/LineChart.svelte';
  import RadioComposer from '$lib/components/RadioComposer.svelte';
  import Icon from '$lib/components/Icon.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const entries = $derived(snap.timing.entries);
  const player = $derived(entries.find((e) => e.isPlayer));
  const fastest = $derived(Math.min(...entries.map((e) => e.bestLapMs ?? Infinity)));
  const bestSectors = $derived([0, 1, 2].map((i) => Math.min(...entries.map((e) => e.sectorsMs[i] ?? Infinity))));
  const rival = $derived(
    entries.find((e) => e.carIndex === pitwall.selectedCar && !e.isPlayer) ?? (player ? entries.find((e) => e.position === player.position - 1) : undefined)
  );

  // Gap trend: rival's gap to leader minus player's (positive = rival behind).
  const trend = $derived.by(() => {
    if (!player || !rival) return [];
    const p = pitwall.gapHist.get(player.carIndex) ?? [];
    const r = pitwall.gapHist.get(rival.carIndex) ?? [];
    const rm = new Map(r.map((s) => [Math.round(s.t / 1000), s.gap]));
    const t0 = p[0]?.t ?? 0;
    return p.flatMap((s) => {
      const rg = rm.get(Math.round(s.t / 1000));
      return rg == null ? [] : [[(s.t - t0) / 60000, (s.gap - rg) / 1000] as [number, number]];
    });
  });

  // Pace delta per lap from the session history.
  const paceSeries = $derived.by(() => {
    if (!player) return [];
    const mine = pitwall.laps.filter((l) => l.carIndex === player.carIndex && l.lapMs > 0);
    const out = [{ label: `${player.driverLabel} (you)`, color: 'var(--player)', points: mine.map((l) => [l.lap, l.lapMs / 1000] as [number, number]) }];
    if (rival) {
      const theirs = pitwall.laps.filter((l) => l.carIndex === rival.carIndex && l.lapMs > 0);
      out.push({ label: rival.driverLabel, color: teamColor(rival.teamId), points: theirs.map((l) => [l.lap, l.lapMs / 1000] as [number, number]) });
    }
    return out;
  });

</script>

{#if mode === 'tile'}
  <table class="tower num">
    <thead>
      <tr><th>P</th><th>DRIVER</th><th class="r">INT</th><th class="r">LAST</th><th>TYRE</th><th></th></tr>
    </thead>
    <tbody>
      {#each entries as e (e.carIndex)}
        <tr class:player={e.isPlayer} class:selected={pitwall.selectedCar === e.carIndex} class:pit={e.pitStatus !== 'none'}>
          <td class="pos">{e.position}</td>
          <td><i class="team" style="background:{teamColor(e.teamId)};color:{teamColor(e.teamId)}"></i>{e.driverLabel}</td>
          <td class="gap">{e.position === 1 ? 'LEADER' : gap(e.gapToAheadMs)}</td>
          <td class="last" class:purple={e.bestLapMs != null && e.lastLapMs === e.bestLapMs && e.bestLapMs === fastest}>{lapTime(e.lastLapMs)}</td>
          <td>
            <span class="tyre">
              {#if e.telemetryRestricted}<span class="faint" title="Restricted telemetry"><Icon name="lock" size={12} /></span>
              {:else if e.compound}<span class="compound" style="color:{COMPOUND_COLOR[e.compound] ?? '#888'}">{e.compound}</span>
                <span class="dim">{e.tyreAgeLaps ?? ''}</span>{/if}
            </span>
          </td>
          <td>
            <span class="tags">
              {#if e.pitStatus !== 'none'}<span class="chip pit-tag">PIT</span>{/if}
              {#if e.penaltiesMs > 0}<span class="chip pen">+{e.penaltiesMs / 1000}s</span>{/if}
              {#if e.pitStops > 0}<span class="faint" title="Pit stops">{e.pitStops}S</span>{/if}
            </span>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{:else}
  <div class="inspect">
    <div class="table-wrap">
      <table class="full num">
        <thead>
          <tr>
            <th>P</th><th>Driver</th><th>Lap</th><th>Interval</th><th>Leader</th><th>Last</th><th>Best</th><th>S1</th><th>S2</th><th>S3</th><th>Tyre</th><th>Stints</th><th>Pit</th><th>Pen</th>
          </tr>
        </thead>
        <tbody>
          {#each entries as e (e.carIndex)}
            <tr
              class:player={e.isPlayer}
              class:selected={pitwall.selectedCar === e.carIndex}
              class:pit={e.pitStatus !== 'none'}
              onclick={() => pitwall.select(e.carIndex)}
            >
              <td class="pos">{e.position}</td>
              <td class="name"><i class="team" style="background:{teamColor(e.teamId)};color:{teamColor(e.teamId)}"></i>{e.driverLabel} <span class="faint">#{e.raceNumber}</span></td>
              <td>{e.lap}</td>
              <td>{e.position === 1 ? '—' : gap(e.gapToAheadMs)}</td>
              <td>{e.position === 1 ? 'LEADER' : gap(e.gapToLeaderMs)}</td>
              <td>{lapTime(e.lastLapMs)}</td>
              <td class:purple={e.bestLapMs === fastest}>{lapTime(e.bestLapMs)}</td>
              {#each e.sectorsMs as s, i}
                <td class:purple={s != null && s === bestSectors[i]}>{sector(s)}</td>
              {/each}
              <td>
                {#if e.telemetryRestricted}<span class="chip" title="Rival telemetry restricted"><Icon name="lock" size={11} /> Restricted</span>
                {:else if e.compound}<span class="compound" style="color:{COMPOUND_COLOR[e.compound]}">{e.compound}</span> <span class="dim">{e.tyreAgeLaps}L</span>{/if}
              </td>
              <td class="stints">
                {#each e.stints as st}<span class="stint" style="border-color:{COMPOUND_COLOR[st.compound] ?? '#555'}">{st.compound} {st.startLap}–{st.endLap ?? ''}</span>{/each}
              </td>
              <td>{e.pitStatus !== 'none' ? 'IN PIT' : e.pitStops}</td>
              <td>{e.penaltiesMs ? `+${e.penaltiesMs / 1000}s` : ''}{e.warnings ? ` ${e.warnings}W` : ''}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="charts">
      <div>
        <div class="label">Gap trend vs {rival?.driverLabel ?? '—'} (s, + = they are behind)</div>
        <LineChart
          series={[{ label: `Gap to ${rival?.driverLabel ?? ''}`, color: 'var(--accent)', points: trend }]}
          xLabel="minutes"
          yFormat={(v) => v.toFixed(1)}
          xFormat={(v) => v.toFixed(1)}
          hLines={[{ y: 0, color: 'var(--text-faint)' }]}
        />
      </div>
      <div>
        <div class="label">Lap times (s)</div>
        <LineChart series={paceSeries} xLabel="lap" yFormat={(v) => v.toFixed(1)} />
      </div>
    </div>
    <RadioComposer
      presets={[
        `Push now, ${rival?.driverLabel ?? 'car ahead'} is in range`,
        'Manage the gap, defend into the braking zones',
        'Box this lap, box this lap',
        'Stay out, stay out'
      ]}
      context={[
        player ? `P${player.position}` : '',
        snap.timing.gapAheadMs != null ? `Gap ahead ${gap(snap.timing.gapAheadMs, '')}s` : '',
        snap.timing.gapBehindMs != null ? `Gap behind ${gap(snap.timing.gapBehindMs, '')}s` : ''
      ].filter(Boolean)}
    />
  </div>
{/if}

<style>
  table {
    width: 100%;
    border-collapse: collapse;
  }
  .tower td {
    padding: 2px 4px;
    font-size: 12px;
    white-space: nowrap;
    border-bottom: 1px solid rgba(110, 255, 200, 0.05);
  }
  .tower .pos {
    color: var(--text);
    font-weight: 700;
  }
  .tower .gap {
    color: var(--data);
  }
  .tower th {
    position: sticky;
    top: -8px;
    z-index: 1;
    background: #04121a;
    font-family: var(--cond);
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.14em;
    color: var(--text-dim);
    text-align: left;
    padding: 2px 4px 4px;
    border-bottom: 1px solid var(--hairline-strong);
  }
  .tower th.r,
  .tower .last {
    text-align: right;
  }
  .tower .last {
    color: var(--text-dim);
  }
  .tower .tyre {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .pos {
    width: 22px;
    text-align: right;
    color: var(--text-dim);
  }
  .gap {
    text-align: right;
  }
  .team {
    display: inline-block;
    width: 4px;
    height: 13px;
    margin-right: 6px;
    vertical-align: -2px;
    border-radius: 0;
    box-shadow: 0 0 5px currentColor;
  }
  tr.player {
    background: linear-gradient(90deg, rgba(255, 225, 77, 0.2), rgba(255, 225, 77, 0.04));
    color: var(--player);
  }
  tr.player .gap {
    color: var(--player);
  }
  tr.selected {
    outline: 1px solid rgba(255, 255, 255, 0.4);
  }
  tr.pit {
    opacity: 0.6;
  }
  .tags {
    display: flex;
    gap: 4px;
    justify-content: flex-end;
    align-items: center;
  }
  .tags .chip {
    padding: 0 4px;
    font-size: 10px;
  }
  .pit-tag {
    color: var(--cold);
    border-color: var(--cold);
  }
  .pen {
    color: var(--crit);
    border-color: var(--crit);
  }
  .tower .compound {
    width: 16px;
    height: 16px;
    font-size: 9px;
    border-width: 2px;
  }
  .inspect {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .table-wrap {
    overflow-x: auto;
  }
  .full th {
    position: sticky;
    top: 0;
    background: var(--surface);
    font-size: 11px;
    font-weight: 600;
    color: var(--text-dim);
    text-align: left;
    padding: 6px;
  }
  .full td {
    padding: 4px 6px;
    font-size: 12.5px;
    border-top: 1px solid var(--hairline);
    white-space: nowrap;
  }
  .full tbody tr {
    cursor: pointer;
  }
  .full tbody tr:hover {
    background: rgba(255, 255, 255, 0.04);
  }
  .full td:nth-child(n + 4):nth-child(-n + 10) {
    color: var(--data);
  }
  .purple {
    color: var(--purple) !important;
    font-weight: 600;
  }
  .stint {
    display: inline-block;
    border: 1px solid;
    border-radius: 4px;
    padding: 0 4px;
    margin-right: 3px;
    font-size: 11px;
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 14px;
  }
</style>
