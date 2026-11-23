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

  // Deep link: /?inspect=tyres opens that monitor full-size.
  onMount(() => {
    const id = new URLSearchParams(location.search).get('inspect');
    if (id && id in MONITORS) pitwall.inspecting = id as MonitorId;
  });
</script>

<svelte:head><title>Pit Wall{pitwall.snap?.session.trackName ? ` · ${pitwall.snap.session.trackName}` : ''}</title></svelte:head>

<div class="app wall">
  <TopBar />
  <main class="stand" style="grid-template-areas: {layout.areas}; grid-template-rows: {layout.rows};">
    <!-- Gantry: two uprights, plus a rail above each monitor row (grid items, so they track the rows). -->
    <div class="gantry" aria-hidden="true"><i class="post left"></i><i class="post right"></i></div>
    {#each [1, 2, 3] as row}<i class="rail" style="grid-row: {row}" aria-hidden="true"></i>{/each}
    {#each ORDER as id (id)}
      {@const def = MONITORS[id]}
      {@const a = pitwall.snap ? def.alert(pitwall.snap) : { level: 'nominal' as const, reason: null }}
      <div class="cell" style="grid-area: {id}">
        <MonitorFrame {id} title={def.title} channel={def.channel} level={a.level} reason={a.reason}>
          {#if pitwall.snap}
            <def.component snap={pitwall.snap} mode="tile" />
          {:else}
            <p class="faint">{pitwall.connected ? 'Waiting for telemetry…' : 'Connecting to pit wall server…'}</p>
          {/if}
        </MonitorFrame>
      </div>
    {/each}
  </main>
  <div class="desk" aria-hidden="true"><span></span></div>
  <Inspector />
</div>

<style>
  .app {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }

  /* Carbon-fibre weave under a top light falloff. */
  .wall {
    background:
      radial-gradient(120% 70% at 50% -10%, rgba(140, 170, 190, 0.12), transparent 60%),
      radial-gradient(90% 60% at 50% 120%, rgba(0, 0, 0, 0.7), transparent 70%),
      linear-gradient(27deg, #0d0f12 5px, transparent 5px) 0 5px / 20px 20px,
      linear-gradient(207deg, #0d0f12 5px, transparent 5px) 10px 0 / 20px 20px,
      linear-gradient(27deg, #16191d 5px, transparent 5px) 0 10px / 20px 20px,
      linear-gradient(207deg, #16191d 5px, transparent 5px) 10px 5px / 20px 20px,
      linear-gradient(90deg, #121417 10px, transparent 10px) 0 0 / 20px 20px,
      linear-gradient(#131518 25%, #101214 25%, #101214 50%, transparent 50%, transparent 75%, #181b1f 75%, #181b1f) 0 0 / 20px 20px,
      #111316;
  }

  .stand {
    position: relative;
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(270px, 1fr) 1.2fr 1.2fr minmax(260px, 1fr);
    gap: 22px 16px;
    padding: 22px 30px 14px;
  }
  .cell {
    position: relative;
    z-index: 1;
    min-height: 0;
    min-width: 0;
    display: flex;
  }
  .cell > :global(*) {
    flex: 1;
  }

  .gantry {
    position: absolute;
    inset: 6px 8px 0;
    pointer-events: none;
    z-index: 0;
  }
  .gantry i {
    position: absolute;
    display: block;
  }
  .rail {
    grid-column: 1 / -1;
    align-self: start;
    z-index: 0;
    height: 12px;
    margin: -17px -22px 0;
    pointer-events: none;
    background: linear-gradient(180deg, #4a515b 0%, #2a2f36 30%, #15181c 70%, #08090b 100%);
    box-shadow:
      0 0 0 1px #000,
      0 6px 12px rgba(0, 0, 0, 0.6);
    border-radius: 2px;
  }
  .post {
    top: 0;
    bottom: 0;
    width: 16px;
    background: linear-gradient(90deg, #08090b 0%, #2e343c 40%, #4a515b 55%, #1a1d22 80%, #08090b 100%);
    box-shadow: 0 0 0 1px #000;
  }
  .post.left {
    left: 0;
  }
  .post.right {
    right: 0;
  }

  /* Desk ledge along the bottom of the wall. */
  .desk {
    flex-shrink: 0;
    height: 20px;
    background: linear-gradient(180deg, #3a4048 0%, #1d2126 18%, #0f1114 60%, #050607 100%);
    box-shadow:
      0 -1px 0 rgba(255, 255, 255, 0.08),
      0 -10px 24px rgba(0, 0, 0, 0.6);
    position: relative;
  }
  .desk span {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--team) 20%, var(--team) 80%, transparent);
    opacity: 0.7;
  }

  @media (max-width: 1100px) {
    .app {
      height: auto;
      min-height: 100vh;
      overflow: visible;
    }
    .stand {
      display: flex;
      flex-direction: column;
      padding: 22px 16px 14px;
    }
    .cell {
      min-height: 280px;
    }
    .rail {
      display: none;
    }
  }
</style>
