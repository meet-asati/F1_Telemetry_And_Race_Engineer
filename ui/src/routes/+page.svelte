<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { LAYOUTS, MONITORS } from '$lib/monitors';
  import MonitorFrame from '$lib/components/MonitorFrame.svelte';
  import TopBar from '$lib/components/TopBar.svelte';
  import Inspector from '$lib/components/Inspector.svelte';
  import type { MonitorId } from '$lib/types';

  import { onMount } from 'svelte';

  const ORDER: MonitorId[] = ['timing', 'track', 'tyres', 'power', 'damage', 'weather', 'comms'];
  const layout = $derived(LAYOUTS[pitwall.layout] ?? LAYOUTS.full);

  // Deep link: /?inspect=tyres opens that monitor's inspector.
  onMount(() => {
    const id = new URLSearchParams(location.search).get('inspect');
    if (id && id in MONITORS) pitwall.inspecting = id as MonitorId;
  });
</script>

<svelte:head><title>Pit Wall{pitwall.snap?.session.trackName ? ` · ${pitwall.snap.session.trackName}` : ''}</title></svelte:head>

<div class="app">
  <TopBar />
  <main class="stand" style="grid-template-areas: {layout.areas}; grid-template-rows: {layout.rows};">
    {#each ORDER as id (id)}
      {@const def = MONITORS[id]}
      {@const a = pitwall.snap ? def.alert(pitwall.snap) : { level: 'nominal' as const, reason: null }}
      <div class="cell" style="grid-area: {id}">
        <MonitorFrame {id} title={def.title} icon={def.icon} level={a.level} reason={a.reason}>
          {#if pitwall.snap}
            <def.component snap={pitwall.snap} mode="tile" />
          {:else}
            <p class="faint">{pitwall.connected ? 'Waiting for telemetry…' : 'Connecting to pit wall server…'}</p>
          {/if}
        </MonitorFrame>
      </div>
    {/each}
  </main>
  <Inspector />
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }
  @media (max-width: 1100px) {
    .app {
      height: auto;
      min-height: 100vh;
      overflow: visible;
    }
  }
  .stand {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(270px, 1fr) 1.2fr 1.2fr minmax(260px, 1fr);
    gap: 10px;
    padding: 10px;
  }
  .cell {
    min-height: 0;
    min-width: 0;
    display: flex;
  }
  .cell > :global(*) {
    flex: 1;
  }
  @media (max-width: 1100px) {
    .stand {
      display: flex;
      flex-direction: column;
      overflow: auto;
    }
    .cell {
      min-height: 260px;
    }
  }
</style>
