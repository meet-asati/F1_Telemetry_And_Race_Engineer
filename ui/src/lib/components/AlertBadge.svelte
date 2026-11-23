<script lang="ts">
  import { levelIcon, levelLabel } from '$lib/format';
  import type { AlertLevel } from '$lib/types';

  let { level, reason = null, compact = false }: { level: AlertLevel; reason?: string | null; compact?: boolean } = $props();
</script>

<span class="badge {level}" title={reason ?? levelLabel(level)}>
  <span aria-hidden="true">{levelIcon(level)}</span>
  {#if !compact || level !== 'nominal'}<span>{levelLabel(level)}</span>{/if}
  {#if reason && level !== 'nominal' && !compact}<span class="reason">· {reason}</span>{/if}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--hairline-strong);
    color: var(--text-dim);
    white-space: nowrap;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .watch {
    color: var(--watch);
    border-color: var(--watch);
    background: var(--watch-bg);
  }
  .critical {
    color: #fff;
    border-color: var(--crit);
    background: var(--crit);
  }
  .nominal {
    color: var(--ok);
    border-color: rgba(45, 212, 191, 0.3);
  }
  .reason {
    font-weight: 500;
    letter-spacing: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
