<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { LAYOUTS, type LayoutPreset } from '$lib/monitors';
  import { clock, weatherIcon } from '$lib/format';

  let showSettings = $state(false);
  let tokenDraft = $state(pitwall.token);

  const s = $derived(pitwall.snap);
  const sess = $derived(s?.session);
  const comms = $derived(s?.comms);
  const source = $derived.by(() => {
    if (!pitwall.connected) return { label: 'Server offline', cls: 'bad' };
    if (!s || s.lastPacketAgeMs == null) return { label: 'Waiting for game', cls: 'idle' };
    if (s.lastPacketAgeMs > 2000) return { label: `No data ${Math.round(s.lastPacketAgeMs / 1000)}s`, cls: 'bad' };
    return { label: { live: 'F1 UDP live', replay: 'Replay', synthetic: 'Synthetic', none: '—' }[s.source], cls: s.source === 'live' ? 'ok' : 'alt' };
  });
  const flag = $derived(sess?.safetyCarStatus === 'full' ? 'SAFETY CAR' : sess?.safetyCarStatus === 'virtual' ? 'VIRTUAL SAFETY CAR' : sess?.marshalZones.some((z) => z.flag === 'red') ? 'RED FLAG' : null);
</script>

<header class="topbar">
  <div class="brand"><span class="logo">◢</span> PIT WALL</div>

  <div class="session">
    {#if sess && sess.trackName}
      <span class="chip strong">{sess.sessionType}</span>
      <span class="num lap">
        {#if sess.isRace}Lap <b>{sess.lap}</b> / {sess.totalLaps}{:else}{clock(sess.timeLeftMs)} left{/if}
      </span>
      <span class="dim">{sess.trackName}</span>
      <span class="num dim">{weatherIcon(sess.weather)} {sess.airTempC}° / {sess.trackTempC}°</span>
      {#if s?.gameYear}<span class="chip">F1 {s.gameYear}</span>{/if}
      {#if s?.timing.position}<span class="chip pos num">P{s.timing.position}</span>{/if}
    {:else}
      <span class="dim">No session</span>
    {/if}
  </div>

  {#if flag}<div class="flag" role="status">⚑ {flag}</div>{/if}

  <div class="right">
    <span class="pill {source.cls}" title="Telemetry source">{source.label}</span>
    <span class="pill {pitwall.connected ? 'ok' : 'bad'}" title="WebSocket">{pitwall.connected ? `WS ${pitwall.msgRate} msg/s` : 'WS reconnecting…'}</span>

    <select value={pitwall.layout} onchange={(e) => pitwall.setLayout((e.currentTarget as HTMLSelectElement).value as LayoutPreset)} aria-label="Layout preset">
      {#each Object.entries(LAYOUTS) as [k, v]}<option value={k}>{v.label}</option>{/each}
    </select>

    {#if comms}
      <div class="seg" role="group" aria-label="Comms mode">
        <button class:on={comms.mode === 'auto'} onclick={() => pitwall.send({ type: 'comms.setMode', mode: 'auto' })}>Auto-Speak</button>
        <button class:on={comms.mode === 'copilot'} onclick={() => pitwall.send({ type: 'comms.setMode', mode: 'copilot' })}>
          Copilot{comms.pendingSuggestions.length ? ` (${comms.pendingSuggestions.length})` : ''}
        </button>
      </div>
      <button class="mute" class:on={comms.muted} onclick={() => pitwall.send({ type: 'comms.mute', muted: !comms.muted })} title="Mute / unmute AI radio">
        {comms.muted ? '🔇 AI muted' : '🔊 AI'}
      </button>
      {#if comms.friendOverrideActive}
        <span class="pill override num" title="AI proactive callouts held while you talk">🎙 Override {Math.ceil(comms.overrideRemainingMs / 1000)}s</span>
      {/if}
    {/if}

    <button class="ghost" onclick={() => (showSettings = !showSettings)} aria-label="Settings" title="Settings">⚙</button>
    <a class="ghost link" href="/driver" target="_blank" title="Open the driver view">Driver view ↗</a>
  </div>
</header>

{#if pitwall.error}<div class="toast" role="alert">{pitwall.error}</div>{/if}

{#if showSettings}
  <div class="settings">
    <label>
      Session token {pitwall.authRequired ? '(required by this server)' : '(not required)'}
      <input type="password" bind:value={tokenDraft} placeholder="shared token" />
    </label>
    <button class="primary" onclick={() => (pitwall.setToken(tokenDraft), (showSettings = false))}>Save</button>
    <span class="faint">Server {pitwall.serverVersion || '—'}</span>
  </div>
{/if}

<style>
  .topbar {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 14px;
    background: var(--bezel);
    border-bottom: 1px solid var(--hairline);
    flex-wrap: wrap;
  }
  .brand {
    font-weight: 800;
    letter-spacing: 0.14em;
    font-size: 13px;
  }
  .logo {
    color: #e10600;
  }
  .session {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .strong {
    font-weight: 700;
    border-color: var(--accent);
    color: var(--accent);
  }
  .lap b {
    font-size: 16px;
  }
  .pos {
    color: var(--player);
    border-color: var(--player);
    font-weight: 700;
  }
  .flag {
    background: #facc15;
    color: #111;
    font-weight: 800;
    padding: 3px 12px;
    border-radius: 4px;
    letter-spacing: 0.08em;
    animation: blink 1s steps(2) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.6;
    }
  }
  .right {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .pill {
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 999px;
    border: 1px solid var(--hairline-strong);
    white-space: nowrap;
  }
  .pill.ok {
    color: var(--ok);
    border-color: rgba(45, 212, 191, 0.5);
  }
  .pill.alt {
    color: #c084fc;
    border-color: rgba(192, 132, 252, 0.5);
  }
  .pill.idle {
    color: var(--text-dim);
  }
  .pill.bad {
    color: var(--crit);
    border-color: var(--crit);
  }
  .pill.override {
    background: #7c3aed;
    border-color: #7c3aed;
    color: #fff;
  }
  .seg {
    display: inline-flex;
  }
  .seg button {
    border-radius: 0;
    font-size: 12px;
  }
  .seg button:first-child {
    border-radius: 6px 0 0 6px;
  }
  .seg button:last-child {
    border-radius: 0 6px 6px 0;
    border-left: none;
  }
  .seg .on {
    background: #134e4a;
    border-color: var(--accent);
    color: var(--accent);
  }
  .mute {
    font-size: 12px;
  }
  .mute.on {
    border-color: var(--watch);
    color: var(--watch);
  }
  .link {
    color: var(--text-dim);
    text-decoration: none;
    font-size: 12px;
  }
  .toast {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--crit);
    color: #fff;
    padding: 8px 14px;
    border-radius: 6px;
    z-index: 60;
  }
  .settings {
    display: flex;
    gap: 10px;
    align-items: flex-end;
    padding: 10px 14px;
    background: var(--surface);
    border-bottom: 1px solid var(--hairline);
  }
  .settings label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-dim);
  }
</style>
