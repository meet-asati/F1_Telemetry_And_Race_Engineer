<script lang="ts">
  import { page } from '$app/state';
  import { pitwall } from '$lib/store.svelte';
  import { MONITORS } from '$lib/monitors';
  import MonitorFrame from '$lib/components/MonitorFrame.svelte';
  import type { MonitorId } from '$lib/types';

  const def = $derived(MONITORS[page.params.id as MonitorId]);
</script>

<svelte:head><title>{def ? def.title : 'Monitor'} · Pit Wall</title></svelte:head>

{#if def}
  {@const a = pitwall.snap ? def.alert(pitwall.snap) : { level: 'nominal' as const, reason: null }}
  <div class="pop">
    <MonitorFrame id={def.id} title={def.title} icon={def.icon} level={a.level} reason={a.reason} standalone>
      {#if pitwall.snap}
        <def.component snap={pitwall.snap} mode="inspect" />
      {:else}
        <p class="faint">{pitwall.connected ? 'Waiting for telemetry…' : 'Connecting…'}</p>
      {/if}
    </MonitorFrame>
  </div>
{:else}
  <p style="padding:20px">Unknown monitor “{page.params.id}”. <a href="/">Back to the stand</a></p>
{/if}

<style>
  .pop {
    height: 100vh;
    display: flex;
    padding: 8px;
  }
  .pop > :global(*) {
    flex: 1;
  }
</style>
