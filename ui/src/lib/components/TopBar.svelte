<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { LAYOUTS, type LayoutPreset } from '$lib/monitors';
  import { clock, weatherGlyph } from '$lib/format';
  import Icon from './Icon.svelte';
  import TeamRadio from './TeamRadio.svelte';

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
  <div class="brand"><span class="logo" aria-hidden="true"></span>PIT WALL <span class="sub">RACE ENGINEERING</span></div>

  <div class="session">
    {#if sess && sess.trackName}
      <span class="chip strong">{sess.sessionType}</span>
      <span class="num lap">
        {#if sess.isRace}Lap <b>{sess.lap}</b> / {sess.totalLaps}{:else}{clock(sess.timeLeftMs)} left{/if}
      </span>
      <span class="dim">{sess.trackName}</span>
      <span class="num dim wx"><Icon name={weatherGlyph(sess.weather)} /> {sess.airTempC}° / {sess.trackTempC}°</span>
      {#if s?.gameYear}<span class="chip">F1 {s.gameYear}</span>{/if}
      {#if s?.timing.position}<span class="chip pos num">P{s.timing.position}</span>{/if}
    {:else}
      <span class="dim">No session</span>
    {/if}
  </div>

  {#if flag}<div class="flag" role="status"><Icon name="flag" /> {flag}</div>{/if}

  <div class="right">
    <TeamRadio />
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
        <Icon name={comms.muted ? 'mute' : 'volume'} />{comms.muted ? 'AI muted' : 'AI'}
      </button>
      {#if comms.friendOverrideActive}
        <span class="pill override num" title="AI proactive callouts held while you talk"><Icon name="mic" size={11} /> Override {Math.ceil(comms.overrideRemainingMs / 1000)}s</span>
      {/if}
    {/if}

    <button class="ghost" onclick={() => (showSettings = !showSettings)} aria-label="Settings" title="Settings"><Icon name="settings" /></button>
    <a class="ghost link" href="/" title="Live pit wall">Stand</a>
    <a class="ghost link" class:ready={sess?.phase === 'finished'} href="/debrief" title="Post-session debrief">
      {sess?.phase === 'finished' ? 'Debrief ready' : 'Debrief'}
    </a>
    <a class="ghost link" href="/driver" target="_blank" title="Open the driver view">Driver view <Icon name="popout" size={12} /></a>
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
    position: relative;
    z-index: 2;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 18px 9px;
    background:
      repeating-linear-gradient(90deg, rgba(255, 255, 255, 0.018) 0 1px, transparent 1px 3px),
      linear-gradient(180deg, #2a2f36 0%, #181b20 45%, #0e1013 100%);
    border-bottom: 2px solid var(--team);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.12),
      0 6px 18px rgba(0, 0, 0, 0.7);
    flex-wrap: wrap;
    font-family: var(--cond);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    letter-spacing: 0.22em;
    font-size: 15px;
  }
  .brand .sub {
    font-size: 9px;
    letter-spacing: 0.3em;
    color: var(--text-dim);
    font-weight: 400;
  }
  .logo {
    width: 18px;
    height: 14px;
    background: var(--team);
    clip-path: polygon(35% 0, 100% 0, 65% 100%, 0 100%);
    box-shadow: 0 0 8px var(--team);
  }
  .wx {
    display: inline-flex;
    align-items: center;
    gap: 5px;
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
    display: inline-flex;
    align-items: center;
    gap: 6px;
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
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    letter-spacing: 0.08em;
    padding: 3px 8px;
    border-radius: 2px;
    background: rgba(0, 0, 0, 0.4);
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
    border-radius: 3px 0 0 3px;
  }
  .seg button:last-child {
    border-radius: 0 3px 3px 0;
    border-left: none;
  }
  .seg .on {
    background: #0b3a44;
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
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--text-dim);
    text-decoration: none;
    font-size: 12px;
    letter-spacing: 0.06em;
  }
  .link:hover {
    color: var(--text);
  }
  .link.ready {
    color: #001a12;
    background: var(--data);
    padding: 2px 8px;
    border-radius: 2px;
    font-weight: 700;
    animation: blink 1.4s steps(2) infinite;
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
