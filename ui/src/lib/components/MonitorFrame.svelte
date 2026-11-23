<script lang="ts">
  import type { Snippet } from 'svelte';
  import AlertBadge from './AlertBadge.svelte';
  import Icon from './Icon.svelte';
  import { pitwall } from '$lib/store.svelte';
  import type { AlertLevel, MonitorId } from '$lib/types';

  let {
    id,
    title,
    channel,
    level = 'nominal',
    reason = null,
    standalone = false,
    children
  }: {
    id: MonitorId;
    title: string;
    channel: string;
    level?: AlertLevel;
    reason?: string | null;
    standalone?: boolean;
    children: Snippet;
  } = $props();

  function open() {
    if (!standalone) pitwall.inspecting = id;
  }

  function popOut(e: MouseEvent) {
    e.stopPropagation();
    window.open(`/monitor/${id}`, `pitwall-${id}`, 'width=1100,height=800');
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      open();
    }
  }
</script>

<!-- Tiles are buttons (open the inspector); the standalone pop-out is a plain region. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<section
  class="mon {level}"
  class:stale={pitwall.stale}
  class:standalone
  role={standalone ? undefined : 'button'}
  tabindex={standalone ? undefined : 0}
  onclick={open}
  onkeydown={standalone ? undefined : onKey}
  aria-label="{title} monitor{standalone ? '' : ', open full view'}"
>
  <div class="mon-screen">
    <header class="mon-head">
      <span class="ch">{channel}</span>
      <h2>{title}</h2>
      <AlertBadge {level} {reason} compact />
      {#if !standalone}
        <button class="icon-btn" title="Pop out to a new window" aria-label="Pop out {title}" onclick={popOut}><Icon name="popout" /></button>
      {/if}
    </header>
    <div class="mon-body">
      {@render children()}
    </div>
    {#if pitwall.stale}
      <div class="stale-tag">{pitwall.waitingForGame ? 'WAITING FOR GAME' : 'NO SIGNAL'}</div>
    {/if}
  </div>
  <div class="mon-chin">PW · {channel}</div>
  <span class="mon-led"></span>
</section>

<style>
  section {
    cursor: pointer;
    transition: transform 0.15s, filter 0.15s;
  }
  section.standalone {
    cursor: default;
  }
  section:not(.standalone):hover {
    filter: brightness(1.12);
  }
  section:not(.standalone):hover :global(.mon-head h2) {
    color: #7ff0ff;
  }
  /* Mounting arm up to the gantry rail. */
  section:not(.standalone)::before {
    content: '';
    position: absolute;
    top: -12px;
    left: 50%;
    width: 46px;
    height: 12px;
    transform: translateX(-50%);
    background: linear-gradient(90deg, #0b0d10, #2a2f36 45%, #15181c);
    border-radius: 2px 2px 0 0;
    box-shadow: 0 0 0 1px #000;
  }
  .stale .mon-body {
    opacity: 0.35;
    filter: grayscale(0.9);
  }
  .stale-tag {
    position: absolute;
    inset: 50% auto auto 50%;
    transform: translate(-50%, -50%);
    z-index: 4;
    font-family: var(--cond);
    font-size: 11px;
    letter-spacing: 0.3em;
    color: var(--text-dim);
    border: 1px solid var(--hairline-strong);
    padding: 3px 10px;
    background: rgba(2, 7, 10, 0.85);
    white-space: nowrap;
  }
</style>
