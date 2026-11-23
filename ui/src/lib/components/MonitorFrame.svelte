<script lang="ts">
  import type { Snippet } from 'svelte';
  import AlertBadge from './AlertBadge.svelte';
  import { pitwall } from '$lib/store.svelte';
  import type { AlertLevel, MonitorId } from '$lib/types';

  let {
    id,
    title,
    icon,
    level = 'nominal',
    reason = null,
    standalone = false,
    children
  }: {
    id: MonitorId;
    title: string;
    icon: string;
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
    window.open(`/monitor/${id}`, `pitwall-${id}`, 'width=1000,height=760');
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      open();
    }
  }
</script>

<!-- Tiles are buttons (open inspector); the standalone pop-out is a plain region. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<section
  class="monitor {level}"
  class:stale={pitwall.stale}
  class:standalone
  role={standalone ? undefined : 'button'}
  tabindex={standalone ? undefined : 0}
  onclick={open}
  onkeydown={standalone ? undefined : onKey}
  aria-label="{title} monitor{standalone ? '' : ', open inspector'}"
>
  <header>
    <span class="icon" aria-hidden="true">{icon}</span>
    <h2>{title}</h2>
    <AlertBadge {level} {reason} compact />
    {#if !standalone}
      <button class="ghost pop" title="Pop out to a new window" aria-label="Pop out {title}" onclick={popOut}>⧉</button>
    {/if}
  </header>
  <div class="body">
    {@render children()}
  </div>
  {#if pitwall.stale}
    <div class="stale-tag">{pitwall.waitingForGame ? 'WAITING FOR GAME' : 'STALE'}</div>
  {/if}
</section>

<style>
  .monitor {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    background: var(--bezel);
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    overflow: hidden;
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .monitor.standalone {
    cursor: default;
  }
  .monitor:not(.standalone):hover {
    border-color: var(--hairline-strong);
  }
  .monitor.watch {
    border-color: var(--watch);
    box-shadow: inset 0 0 0 1px var(--watch);
  }
  .monitor.critical {
    border-color: var(--crit);
    box-shadow: inset 0 0 0 1px var(--crit), 0 0 18px rgba(239, 68, 68, 0.25);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      box-shadow: inset 0 0 0 1px var(--crit), 0 0 4px rgba(239, 68, 68, 0.1);
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border-bottom: 1px solid var(--hairline);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.02), transparent);
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .icon {
    font-size: 14px;
  }
  .pop {
    padding: 0 6px;
    font-size: 14px;
    border: none;
    color: var(--text-dim);
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 10px;
    overflow: auto;
  }
  .stale .body {
    opacity: 0.4;
    filter: grayscale(0.8);
  }
  .stale-tag {
    position: absolute;
    right: 10px;
    bottom: 8px;
    font-size: 10px;
    letter-spacing: 0.1em;
    color: var(--text-dim);
    border: 1px solid var(--hairline-strong);
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--canvas);
  }
</style>
