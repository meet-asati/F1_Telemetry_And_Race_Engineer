<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import Icon from './Icon.svelte';

  let {
    presets = [],
    context = [],
    placeholder = 'Message to the driver…',
    compact = false
  }: { presets?: string[]; context?: string[]; placeholder?: string; compact?: boolean } = $props();

  let text = $state('');
  let attached = $state<string[]>([]);
  let sentFlash = $state('');

  function toggle(c: string) {
    attached = attached.includes(c) ? attached.filter((x) => x !== c) : [...attached, c];
  }

  function send(msg: string) {
    if (pitwall.radio(msg, attached)) {
      sentFlash = msg;
      text = '';
      attached = [];
      setTimeout(() => (sentFlash = ''), 2500);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send(text);
    }
  }
</script>

<div class="composer" class:compact>
  {#if presets.length}
    <div class="presets">
      {#each presets as p}
        <button onclick={() => send(p)} title="Send now"><Icon name="radio" size={12} /> {p}</button>
      {/each}
    </div>
  {/if}
  {#if context.length}
    <div class="context">
      <span class="label">Attach</span>
      {#each context as c}
        <button class="chip" class:on={attached.includes(c)} onclick={() => toggle(c)} aria-pressed={attached.includes(c)}>{c}</button>
      {/each}
    </div>
  {/if}
  <div class="row">
    <input bind:value={text} onkeydown={onKey} {placeholder} aria-label="Radio message" />
    <button class="primary" disabled={!text.trim()} onclick={() => send(text)}>Send to radio</button>
  </div>
  {#if sentFlash}<div class="sent"><Icon name="check" size={12} /> Sent: “{sentFlash}”</div>{/if}
</div>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: rgba(4, 16, 22, 0.6);
  }
  .presets,
  .context {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .presets button {
    font-size: 12px;
  }
  .chip {
    background: transparent;
    padding: 2px 8px;
  }
  .chip.on {
    border-color: var(--accent);
    color: var(--accent);
    background: rgba(94, 234, 212, 0.1);
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .row input {
    flex: 1;
    min-width: 0;
  }
  .sent {
    font-size: 12px;
    color: var(--ok);
  }
</style>
