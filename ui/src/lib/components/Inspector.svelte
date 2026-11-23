<script lang="ts">
  import { fade, scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { pitwall } from '$lib/store.svelte';
  import { MONITORS } from '$lib/monitors';
  import AlertBadge from './AlertBadge.svelte';
  import Icon from './Icon.svelte';
  import type { TelemetrySnapshot } from '$lib/types';

  let live = $state(true);
  let frozen = $state.raw<TelemetrySnapshot | null>(null);
  let closeBtn = $state<HTMLButtonElement | null>(null);
  const def = $derived(pitwall.inspecting ? MONITORS[pitwall.inspecting] : null);
  const snap = $derived(live ? pitwall.snap : frozen);
  const alert = $derived(def && snap ? def.alert(snap) : { level: 'nominal' as const, reason: null });

  $effect(() => {
    if (def) closeBtn?.focus();
  });

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
  <div class="scrim" role="presentation" onclick={close} transition:fade={{ duration: 150 }}></div>
  <div class="modal" role="dialog" aria-modal="true" aria-label="{def.title} full view" transition:scale={{ start: 0.93, duration: 200, easing: cubicOut }}>
    <div class="mon big {alert.level}">
      <div class="mon-screen">
        <header class="mon-head">
          <span class="ch">{def.channel}</span>
          <h2>{def.title}</h2>
          <AlertBadge level={alert.level} reason={alert.reason} />
          <span class="spacer"></span>
          <button class="live" class:frozen={!live} onclick={toggleLive} title="Freeze the view to study a moment, or follow live data">
            <span class="dot"></span>{live ? 'LIVE 15 HZ' : 'FROZEN'}
          </button>
          <button onclick={() => window.open(`/monitor/${def.id}`, `pitwall-${def.id}`, 'width=1100,height=800')}><Icon name="popout" /> Pop out</button>
          <button bind:this={closeBtn} onclick={close} aria-label="Close"><Icon name="close" /> Close</button>
        </header>
        <div class="mon-body content">
          {#if snap}
            <def.component {snap} mode="inspect" />
          {:else}
            <p class="faint">No telemetry yet.</p>
          {/if}
        </div>
      </div>
      <div class="mon-chin">PIT WALL · CH {def.channel} · {def.title.toUpperCase()}</div>
      <span class="mon-led"></span>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: radial-gradient(ellipse at center, rgba(0, 0, 0, 0.55), rgba(0, 0, 0, 0.85));
    backdrop-filter: blur(3px);
    z-index: 40;
  }
  .modal {
    position: fixed;
    inset: 3.5vh 3vw;
    z-index: 41;
    display: flex;
    pointer-events: none;
  }
  .mon.big {
    flex: 1;
    max-width: 1500px;
    margin: 0 auto;
    padding: 12px 12px 22px;
    border-radius: 12px;
    pointer-events: auto;
    box-shadow:
      0 0 0 1px #000,
      inset 0 1px 0 rgba(255, 255, 255, 0.16),
      0 40px 90px rgba(0, 0, 0, 0.85),
      0 0 60px rgba(34, 211, 238, 0.06);
  }
  .mon.big .mon-head {
    padding: 8px 12px;
    flex-wrap: wrap;
  }
  .mon.big .mon-head h2 {
    font-size: 15px;
    flex: 0 1 auto;
  }
  .mon.big .mon-chin {
    font-size: 9px;
    height: 20px;
  }
  .mon.big .mon-led {
    bottom: 9px;
    right: 20px;
  }
  .spacer {
    flex: 1;
  }
  .live {
    color: var(--ok);
    font-size: 12px;
  }
  .live .dot {
    width: 7px;
    height: 7px;
    box-shadow: 0 0 6px currentColor;
    animation: blink 1.2s steps(2) infinite;
  }
  .live.frozen {
    border-color: var(--watch);
    color: var(--watch);
  }
  .live.frozen .dot {
    animation: none;
  }
  @keyframes blink {
    50% {
      opacity: 0.3;
    }
  }
  .content {
    padding: 16px 18px;
  }
  @media (max-width: 700px) {
    .modal {
      inset: 0;
    }
    .mon.big {
      border-radius: 0;
    }
  }
</style>
