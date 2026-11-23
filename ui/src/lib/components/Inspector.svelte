<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { MONITORS } from '$lib/monitors';
  import AlertBadge from './AlertBadge.svelte';
  import type { TelemetrySnapshot } from '$lib/types';

  let live = $state(true);
  let frozen = $state.raw<TelemetrySnapshot | null>(null);
  const def = $derived(pitwall.inspecting ? MONITORS[pitwall.inspecting] : null);
  const snap = $derived(live ? pitwall.snap : frozen);

  function close() {
    pitwall.inspecting = null;
    live = true;
    frozen = null;
  }
  function toggleLive() {
    if (live) frozen = pitwall.snap;
    live = !live;
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && pitwall.inspecting) close();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if def}
  <div class="scrim" role="presentation" onclick={close}></div>
  <div class="drawer" role="dialog" aria-modal="true" aria-label="{def.title} inspector">
    <header>
      <span class="icon">{def.icon}</span>
      <h2>{def.title}</h2>
      {#if snap}
        {@const a = def.alert(snap)}
        <AlertBadge level={a.level} reason={a.reason} />
      {/if}
      <span class="spacer"></span>
      <button class:frozen={!live} onclick={toggleLive} title="Freeze the view to study a moment, or follow live data">
        {live ? '● Live 15 Hz' : '❚❚ Snapshot (frozen)'}
      </button>
      <button onclick={() => window.open(`/monitor/${def.id}`, `pitwall-${def.id}`, 'width=1100,height=800')}>⧉ Pop out</button>
      <button onclick={close} aria-label="Close inspector">✕</button>
    </header>
    <div class="content">
      {#if snap}
        <def.component {snap} mode="inspect" />
      {:else}
        <p class="faint">No telemetry yet.</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(5, 8, 14, 0.6);
    z-index: 40;
  }
  .drawer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(1100px, 92vw);
    background: var(--surface);
    border-left: 1px solid var(--hairline-strong);
    z-index: 41;
    display: flex;
    flex-direction: column;
    box-shadow: -20px 0 50px rgba(0, 0, 0, 0.5);
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--hairline);
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .icon {
    font-size: 18px;
  }
  .spacer {
    flex: 1;
  }
  .frozen {
    border-color: var(--watch);
    color: var(--watch);
  }
  .content {
    flex: 1;
    overflow: auto;
    padding: 16px;
  }
  @media (max-width: 700px) {
    .drawer {
      width: 100vw;
    }
  }
</style>
