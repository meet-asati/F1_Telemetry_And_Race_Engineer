<script lang="ts">
  import { levelLabel } from '$lib/format';
  import type { AlertLevel } from '$lib/types';

  let { level, reason = null, compact = false }: { level: AlertLevel; reason?: string | null; compact?: boolean } = $props();
</script>

<!-- Shape + text + colour, so the state never relies on colour alone. -->
<span class="badge {level}" title={reason ?? levelLabel(level)}>
  <svg width="8" height="8" viewBox="0 0 8 8" aria-hidden="true">
    {#if level === 'critical'}<rect width="8" height="8" fill="currentColor" />
    {:else if level === 'watch'}<path d="M4 0L8 8H0z" fill="currentColor" />
    {:else}<circle cx="4" cy="4" r="3.5" fill="currentColor" />{/if}
  </svg>
  {#if !compact || level !== 'nominal'}<span>{levelLabel(level)}</span>{/if}
  {#if reason && level !== 'nominal' && !compact}<span class="reason">· {reason}</span>{/if}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-family: var(--cond);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.1em;
    padding: 1px 7px;
    border-radius: 2px;
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
    border-color: rgba(61, 220, 132, 0.3);
  }
  .reason {
    font-weight: 500;
    letter-spacing: 0.02em;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
