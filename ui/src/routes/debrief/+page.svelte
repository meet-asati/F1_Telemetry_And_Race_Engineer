<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '$lib/components/TopBar.svelte';
  import LineChart, { type Series } from '$lib/components/LineChart.svelte';
  import { COMPOUND_COLOR, lapTime, sector, teamColor } from '$lib/format';
  import type { DebriefResponse, LapRow, RadioEvent, SessionListItem } from '$lib/types';

  let sessions = $state<SessionListItem[]>([]);
  let sessionId = $state<number | null>(null);
  let car = $state<number | null>(null);
  let data = $state.raw<DebriefResponse | null>(null);
  let laps = $state.raw<LapRow[]>([]);
  let radio = $state.raw<{ atMs: number; priority: string; source: RadioEvent['source']; text: string; outcome: string }[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(false);

  onMount(async () => {
    const q = new URLSearchParams(location.search);
    try {
      sessions = await (await fetch('/api/sessions')).json();
    } catch {
      error = 'Pit wall server not reachable.';
      return;
    }
    const wanted = Number(q.get('session'));
    const first = sessions.find((s) => s.id === wanted) ?? sessions.find((s) => s.lapCount > 0);
    if (q.get('car') != null) car = Number(q.get('car'));
    if (first) await load(first.id, car);
  });

  async function load(id: number, carIndex: number | null) {
    loading = true;
    error = null;
    sessionId = id;
    try {
      const url = `/api/sessions/${id}/debrief${carIndex != null ? `?car=${carIndex}` : ''}`;
      const r = await fetch(url);
      if (!r.ok) {
        data = null;
        error = r.status === 404 ? 'No laps recorded for this session yet.' : `Server error ${r.status}`;
        return;
      }
      data = await r.json();
      car = data!.debrief.carIndex;
      const [l, rad] = await Promise.all([fetch(`/api/sessions/${id}/laps`).then((x) => x.json()), fetch(`/api/sessions/${id}/radio`).then((x) => x.json())]);
      laps = l;
      radio = rad;
      history.replaceState(null, '', `/debrief?session=${id}&car=${car}`);
    } catch {
      error = 'Could not load the debrief.';
    } finally {
      loading = false;
    }
  }

  const d = $derived(data?.debrief ?? null);
  const s = $derived(data?.session ?? null);
  const mine = $derived(d ? laps.filter((l) => l.carIndex === d.carIndex).sort((a, b) => a.lap - b.lap) : []);
  const kindOf = $derived(new Map(d?.lapKinds.map((k) => [k.lap, k.kind]) ?? []));
  const winner = $derived(d?.field.find((f) => f.finishPosition === 1 && f.carIndex !== d.carIndex) ?? null);
  const ahead = $derived(d && d.result.finishPosition > 1 ? d.field.find((f) => f.finishPosition === d.result.finishPosition - 1 && f.carIndex !== winner?.carIndex) : null);

  // Lap times, with slow laps (pit, safety car) clipped out so the scale stays useful.
  const lapSeries = $derived.by((): Series[] => {
    if (!d) return [];
    const cap = (d.pace.medianLapMs ?? Infinity) * 1.07;
    const line = (ci: number, label: string, color: string): Series => ({
      label,
      color,
      points: laps.filter((l) => l.carIndex === ci && l.lapMs > 0 && l.lapMs <= cap).sort((a, b) => a.lap - b.lap).map((l) => [l.lap, l.lapMs / 1000])
    });
    const out = [line(d.carIndex, `${d.driver} (you)`, 'var(--player)')];
    if (winner) out.push(line(winner.carIndex, `${winner.driver} (P1)`, 'var(--purple)'));
    if (ahead) out.push(line(ahead.carIndex, `${ahead.driver} (P${ahead.finishPosition})`, 'var(--accent)'));
    return out;
  });
  const positionSeries = $derived<Series[]>(d ? [{ label: 'Position', color: 'var(--player)', points: mine.map((l) => [l.lap, -l.position]) }] : []);
  const wearSeries = $derived<Series[]>(
    d ? [{ label: 'Worst-corner wear %', color: 'var(--watch)', points: mine.filter((l) => l.wear).map((l) => [l.lap, Math.max(...l.wear!)]) }] : []
  );
  const fuelSeries = $derived<Series[]>(d ? [{ label: 'Fuel kg', color: 'var(--accent)', points: mine.filter((l) => l.fuelKg != null).map((l) => [l.lap, l.fuelKg!]) }] : []);
  const pitLaps = $derived(d?.lapKinds.filter((k) => k.kind === 'inLap').map((k) => k.lap) ?? []);

  const ordinal = (n: number) => `${n}${n % 100 >= 11 && n % 100 <= 13 ? 'th' : ['th', 'st', 'nd', 'rd'][n % 10] ?? 'th'}`;
  const secs = (ms: number | null) => (ms == null ? '—' : `${(ms / 1000).toFixed(2)}s`);
  const KIND_LABEL: Record<string, string> = { clean: '', firstLap: 'L1', inLap: 'IN', outLap: 'OUT', invalid: 'INV', slow: 'SLOW' };
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
</script>

<svelte:head><title>Debrief{s ? ` · ${s.trackName}` : ''} · Pit Wall</title></svelte:head>

<div class="page wall">
  <TopBar />
  <div class="controls">
    <span class="title">POST-SESSION DEBRIEF</span>
    <select value={sessionId ?? ''} onchange={(e) => load(Number((e.currentTarget as HTMLSelectElement).value), null)} aria-label="Session">
      {#each sessions as x}
        <option value={x.id}>{x.trackName} · {x.sessionType} · {x.startedAt.slice(0, 16)} ({x.lapCount} laps)</option>
      {/each}
    </select>
    {#if d}
      <select value={car ?? ''} onchange={(e) => sessionId != null && load(sessionId, Number((e.currentTarget as HTMLSelectElement).value))} aria-label="Driver">
        {#each [...d.field].sort((a, b) => a.finishPosition - b.finishPosition) as f}
          <option value={f.carIndex}>P{f.finishPosition} {f.driver}{f.carIndex === s?.playerCar ? ' (you)' : ''}</option>
        {/each}
      </select>
    {/if}
    {#if loading}<span class="faint">Loading…</span>{/if}
  </div>

  {#if error}
    <p class="empty">{error}</p>
  {:else if d && s}
    <main class="grid">
      <!-- Result -->
      <section class="mon result" style="grid-area: result">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">R</span><h2>{d.isRace ? 'Result' : 'Session'}</h2></header>
          <div class="mon-body">
            <div class="hero">
              <div class="big num">P{d.result.finishPosition}</div>
              <div class="hero-side">
                <div class="who">{d.driver}</div>
                <div class="dim">{s.trackName} · {s.sessionType} · F1 {s.gameYear}</div>
                {#if d.isRace}
                  <div class="num" class:up={d.result.positionsGained > 0} class:down={d.result.positionsGained < 0}>
                    {d.result.positionsGained > 0 ? `+${d.result.positionsGained}` : d.result.positionsGained} from P{d.result.lap1Position} on lap 1
                  </div>
                {/if}
              </div>
            </div>
            <div class="kpis num">
              <div><span class="k">Best lap</span><b class="purple">{lapTime(d.pace.bestLapMs)}</b><span class="dim">{d.pace.bestLapRank ? ordinal(d.pace.bestLapRank) : ''}</span></div>
              <div><span class="k">Race pace</span><b>{lapTime(d.pace.medianLapMs)}</b><span class="dim">{d.pace.medianRank ? ordinal(d.pace.medianRank) : ''}</span></div>
              <div><span class="k">Ideal lap</span><b>{lapTime(d.pace.theoreticalBestMs)}</b></div>
              <div><span class="k">Consistency</span><b>{secs(d.pace.consistencyMs)}</b></div>
              <div><span class="k">Laps / stops</span><b>{d.result.lapsCompleted} / {d.result.pitStops}</b></div>
              {#if d.fuel}<div><span class="k">Fuel at flag</span><b>{d.fuel.endKg.toFixed(1)} kg</b><span class="dim">{d.fuel.avgBurnKg.toFixed(2)} kg/lap</span></div>{/if}
            </div>
          </div>
        </div>
        <div class="mon-chin">PW · DEBRIEF</div>
        <span class="mon-led"></span>
      </section>

      <!-- Engineer -->
      <section class="mon" style="grid-area: engineer">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">E</span><h2>Engineer's debrief</h2></header>
          <div class="mon-body">
            <ul class="insights">
              {#each d.insights as i}
                <li><span class="tag {i.kind}">{i.kind.toUpperCase()}</span>{i.text}</li>
              {:else}
                <li class="faint">Not enough clean laps for observations.</li>
              {/each}
            </ul>
          </div>
        </div>
        <div class="mon-chin">PW · ENGINEER</div>
        <span class="mon-led"></span>
      </section>

      <!-- Lap times -->
      <section class="mon" style="grid-area: laps">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">01</span><h2>Lap times</h2></header>
          <div class="mon-body">
            <LineChart series={lapSeries} height={200} xLabel="lap" yFormat={(v) => v.toFixed(1)} />
          </div>
        </div>
        <div class="mon-chin">PW · 01</div>
        <span class="mon-led"></span>
      </section>

      <!-- Position -->
      <section class="mon" style="grid-area: position">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">02</span><h2>Position</h2>{#if pitLaps.length}<span class="faint small">pit: L{pitLaps.join(', L')}</span>{/if}</header>
          <div class="mon-body">
            <LineChart series={positionSeries} height={200} xLabel="lap" yFormat={(v) => (Math.abs(v - Math.round(v)) < 0.2 ? `P${Math.round(-v)}` : '')} />
          </div>
        </div>
        <div class="mon-chin">PW · 02</div>
        <span class="mon-led"></span>
      </section>

      <!-- Stints -->
      <section class="mon" style="grid-area: stints">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">03</span><h2>Stints &amp; tyres</h2></header>
          <div class="mon-body">
            <table class="num">
              <thead><tr><th>Tyre</th><th>Laps</th><th class="r">Avg</th><th class="r">Deg /lap</th><th class="r">Wear /lap</th><th class="r">End wear</th></tr></thead>
              <tbody>
                {#each d.stints as st}
                  <tr>
                    <td><span class="compound" style="color:{COMPOUND_COLOR[st.compound] ?? '#888'}">{st.compound}</span></td>
                    <td>L{st.startLap}–L{st.endLap} <span class="faint">({st.laps})</span></td>
                    <td class="r">{lapTime(st.avgLapMs)}</td>
                    <td class="r" class:bad={(st.degMsPerLap ?? 0) >= 120}>{st.degMsPerLap != null ? `${st.degMsPerLap >= 0 ? '+' : ''}${(st.degMsPerLap / 1000).toFixed(3)}s` : '—'}</td>
                    <td class="r">{st.wearPerLap != null ? `${st.wearPerLap.toFixed(1)}%` : '—'}</td>
                    <td class="r">{st.endWear != null ? `${st.endWear.toFixed(0)}%` : '—'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <div class="two">
              <LineChart series={wearSeries} height={120} xLabel="lap" yMin={0} hLines={[{ y: 70, color: 'var(--crit)', label: 'limit' }]} />
              {#if fuelSeries[0].points.length}<LineChart series={fuelSeries} height={120} xLabel="lap" yMin={0} yFormat={(v) => v.toFixed(0)} />{/if}
            </div>
          </div>
        </div>
        <div class="mon-chin">PW · 03</div>
        <span class="mon-led"></span>
      </section>

      <!-- Sectors -->
      <section class="mon" style="grid-area: sectors">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">04</span><h2>Sectors</h2></header>
          <div class="mon-body">
            <table class="num">
              <thead><tr><th></th><th class="r">Your best</th><th class="r">Field best</th><th class="r">Gap</th></tr></thead>
              <tbody>
                {#each [0, 1, 2] as i}
                  {@const mineS = d.pace.bestSectorsMs[i]}
                  {@const fieldS = d.pace.fieldBestSectorsMs[i]}
                  {@const gap = mineS != null && fieldS != null ? mineS - fieldS : null}
                  <tr>
                    <td>S{i + 1}</td>
                    <td class="r">{sector(mineS)}</td>
                    <td class="r purple">{sector(fieldS)}</td>
                    <td class="r" class:bad={(gap ?? 0) >= 150} class:good={gap === 0}>{gap == null ? '—' : gap === 0 ? 'BEST' : `+${(gap / 1000).toFixed(3)}`}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <div class="laps-strip">
              {#each mine as l}
                {@const k = kindOf.get(l.lap) ?? 'clean'}
                <span class="lapcell {k}" title="Lap {l.lap}: {lapTime(l.lapMs)} {KIND_LABEL[k]}">
                  <span class="faint">L{l.lap}</span>{lapTime(l.lapMs)}{#if KIND_LABEL[k]}<i>{KIND_LABEL[k]}</i>{/if}
                </span>
              {/each}
            </div>
          </div>
        </div>
        <div class="mon-chin">PW · 04</div>
        <span class="mon-led"></span>
      </section>

      <!-- Field -->
      <section class="mon" style="grid-area: field">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">05</span><h2>Field pace</h2></header>
          <div class="mon-body">
            <table class="num field">
              <thead><tr><th>P</th><th>Driver</th><th class="r">Best</th><th class="r">Race pace</th><th class="r">Stops</th></tr></thead>
              <tbody>
                {#each d.field as f (f.carIndex)}
                  <tr class:me={f.carIndex === d.carIndex} onclick={() => sessionId != null && load(sessionId, f.carIndex)}>
                    <td>{f.finishPosition}</td>
                    <td>{f.driver}{f.carIndex === s.playerCar ? ' (you)' : ''}</td>
                    <td class="r" class:purple={f.bestLapMs != null && f.bestLapMs === Math.min(...d.field.map((x) => x.bestLapMs ?? Infinity))}>{lapTime(f.bestLapMs)}</td>
                    <td class="r">{lapTime(f.medianLapMs)}</td>
                    <td class="r">{f.pitStops}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
        <div class="mon-chin">PW · 05</div>
        <span class="mon-led"></span>
      </section>

      <!-- Radio -->
      <section class="mon" style="grid-area: radio">
        <div class="mon-screen">
          <header class="mon-head"><span class="ch">06</span><h2>Radio log</h2></header>
          <div class="mon-body">
            <ul class="radio">
              {#each radio as r}
                <li class="p-{r.priority}"><span class="faint">{time(r.atMs)}</span><b class="src">{r.source.toUpperCase()}</b>{r.text}</li>
              {:else}
                <li class="faint">No radio traffic recorded.</li>
              {/each}
            </ul>
          </div>
        </div>
        <div class="mon-chin">PW · 06</div>
        <span class="mon-led"></span>
      </section>
    </main>
  {:else if !loading}
    <p class="empty">No sessions recorded yet. Drive a session with the pit wall running, then come back.</p>
  {/if}
</div>

<style>
  .page {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    background:
      radial-gradient(120% 70% at 50% -10%, rgba(140, 170, 190, 0.12), transparent 60%),
      linear-gradient(27deg, #0d0f12 5px, transparent 5px) 0 5px / 20px 20px,
      linear-gradient(207deg, #0d0f12 5px, transparent 5px) 10px 0 / 20px 20px,
      linear-gradient(27deg, #16191d 5px, transparent 5px) 0 10px / 20px 20px,
      linear-gradient(207deg, #16191d 5px, transparent 5px) 10px 5px / 20px 20px,
      #111316;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
    padding: 14px 24px 4px;
  }
  .title {
    font-family: var(--cond);
    font-weight: 700;
    letter-spacing: 0.2em;
    color: var(--accent);
    margin-right: 8px;
  }
  .controls select {
    max-width: 100%;
  }
  .warn {
    color: var(--watch);
    border-color: var(--watch);
  }
  .empty {
    margin: 60px auto;
    color: var(--text-dim);
  }
  .grid {
    display: grid;
    gap: 22px 16px;
    padding: 22px 24px 30px;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    grid-template-areas:
      'result result engineer'
      'laps laps position'
      'stints stints sectors'
      'field field radio';
  }
  @media (max-width: 1100px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
      grid-template-areas: 'result' 'engineer' 'laps' 'position' 'stints' 'sectors' 'field' 'radio';
      padding: 16px;
    }
  }
  .hero {
    display: flex;
    gap: 18px;
    align-items: center;
    margin-bottom: 12px;
  }
  .hero .big {
    font-size: 64px;
    font-weight: 800;
    color: var(--player);
    line-height: 1;
    text-shadow: 0 0 18px rgba(255, 225, 77, 0.35);
  }
  .who {
    font-size: 20px;
    font-weight: 700;
  }
  .up {
    color: var(--data);
  }
  .down {
    color: var(--crit);
  }
  .kpis {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 8px;
  }
  .kpis > div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
    border: 1px solid var(--hairline);
    background: rgba(4, 16, 22, 0.6);
  }
  .kpis b {
    font-size: 18px;
    color: var(--data);
  }
  .k {
    font-family: var(--cond);
    font-size: 10px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--text-dim);
  }
  .purple {
    color: var(--purple) !important;
  }
  .insights,
  .radio {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .insights li {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-size: 14.5px;
    line-height: 1.35;
  }
  .tag {
    flex-shrink: 0;
    width: 92px;
    font-family: var(--cond);
    font-size: 10px;
    letter-spacing: 0.14em;
    color: var(--accent);
  }
  .tag.result {
    color: var(--player);
  }
  .tag.tyres,
  .tag.fuel {
    color: var(--watch);
  }
  .small {
    font-size: 11px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12.5px;
  }
  th {
    font-family: var(--cond);
    font-size: 10px;
    letter-spacing: 0.12em;
    color: var(--text-dim);
    text-align: left;
    font-weight: 600;
    padding: 4px 6px;
    border-bottom: 1px solid var(--hairline-strong);
  }
  td {
    padding: 4px 6px;
    border-bottom: 1px solid rgba(110, 255, 200, 0.05);
  }
  .r {
    text-align: right;
  }
  .bad {
    color: var(--crit);
  }
  .good {
    color: var(--purple);
  }
  .field tbody tr {
    cursor: pointer;
  }
  .field tbody tr:hover {
    background: rgba(255, 255, 255, 0.04);
  }
  tr.me {
    background: linear-gradient(90deg, rgba(255, 225, 77, 0.18), rgba(255, 225, 77, 0.03));
    color: var(--player);
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 10px;
    margin-top: 10px;
  }
  .laps-strip {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 12px;
  }
  .lapcell {
    font-family: var(--mono);
    font-size: 11px;
    padding: 2px 5px;
    border: 1px solid var(--hairline);
    display: inline-flex;
    gap: 4px;
  }
  .lapcell.clean {
    color: var(--data);
  }
  .lapcell.inLap,
  .lapcell.outLap {
    color: var(--cold);
  }
  .lapcell.slow,
  .lapcell.invalid {
    color: var(--text-faint);
  }
  .lapcell i {
    font-style: normal;
    font-size: 9px;
    color: var(--watch);
  }
  .radio {
    max-height: 340px;
    overflow: auto;
    gap: 4px;
    font-size: 13px;
  }
  .radio li {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .radio .src {
    font-family: var(--cond);
    font-size: 10px;
    letter-spacing: 0.1em;
    color: var(--accent);
    width: 64px;
    flex-shrink: 0;
  }
  .radio .p-P0 {
    color: var(--watch);
  }
</style>
