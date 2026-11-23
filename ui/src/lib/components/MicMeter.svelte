<script lang="ts">
  // Microphone input level meter: segmented bar, green → amber → red.
  let { level = 0, compact = false }: { level?: number; compact?: boolean } = $props();

  const SEGMENTS = 20;
  // Perceptual scale: speech peaks ~0.1–0.5 should fill most of the bar.
  const lit = $derived(Math.round(Math.min(1, Math.sqrt(Math.max(0, level)) * 1.25) * SEGMENTS));
</script>

<div class="meter" class:compact role="meter" aria-label="Microphone level" aria-valuemin="0" aria-valuemax="1" aria-valuenow={level}>
  {#each { length: SEGMENTS } as _, i}
    <i class:on={i < lit} class:hot={i >= SEGMENTS * 0.7} class:clip={i >= SEGMENTS * 0.9}></i>
  {/each}
</div>

<style>
  .meter {
    display: flex;
    gap: 2px;
    height: 12px;
    align-items: stretch;
  }
  .meter.compact {
    height: 8px;
    gap: 1px;
  }
  i {
    flex: 1;
    background: rgba(110, 255, 200, 0.08);
    border-radius: 1px;
  }
  i.on {
    background: var(--data);
    box-shadow: 0 0 4px var(--data);
  }
  i.on.hot {
    background: var(--watch);
    box-shadow: 0 0 4px var(--watch);
  }
  i.on.clip {
    background: var(--crit);
    box-shadow: 0 0 4px var(--crit);
  }
</style>
