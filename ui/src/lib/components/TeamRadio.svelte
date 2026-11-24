<script lang="ts">
  // Duo-mode radio controls: join the radio channel, hold to talk to the driver
  // (button, or hold T anywhere outside a text field).
  import { pitwall } from '$lib/store.svelte';
  import { teamRadio as r } from '$lib/teamRadio.svelte';
  import Icon from './Icon.svelte';
  import MicMeter from './MicMeter.svelte';

  let { large = false }: { large?: boolean } = $props();

  const c = $derived(pitwall.snap?.comms);
  const driverOnAir = $derived(!!c?.pttActive || r.hearing === 'driver');
  const busy = $derived(!!c?.engineerTx && !r.talking);

  function typing(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    return !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable);
  }
  function keydown(e: KeyboardEvent) {
    if ((e.key === 't' || e.key === 'T') && !e.repeat && !typing(e) && !large) r.startTalk();
  }
  function keyup(e: KeyboardEvent) {
    if ((e.key === 't' || e.key === 'T') && !large) r.stopTalk();
  }
  function down(e: PointerEvent) {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    r.startTalk();
  }
</script>

<svelte:window onkeydown={keydown} onkeyup={keyup} onblur={() => r.stopTalk()} />

<div class="radio" class:large>
  <button class="join" class:on={r.on} onclick={() => r.toggle()} disabled={r.connecting} title={r.on ? 'Leave the team radio' : 'Listen to the team radio'}>
    <Icon name={r.on ? 'volume' : 'mute'} size={large ? 16 : 13} />
    {r.on ? 'RADIO' : r.connecting ? '…' : 'RADIO OFF'}
  </button>
  <button
    class="talk"
    class:live={r.talking}
    disabled={busy}
    title="Hold to talk to the driver (or hold T)"
    onpointerdown={down}
    onpointerup={() => r.stopTalk()}
    onpointercancel={() => r.stopTalk()}
    onlostpointercapture={() => r.stopTalk()}
    oncontextmenu={(e) => e.preventDefault()}
  >
    <Icon name="mic" size={large ? 16 : 13} />
    {r.talking ? 'ON AIR' : busy ? 'ENGINEER ON AIR' : 'TALK'}
  </button>
  {#if r.talking}<span class="meter"><MicMeter level={r.micLevel} compact /></span>{/if}
  {#if driverOnAir}<span class="state driver">DRIVER</span>{:else if r.hearing === 'engineer'}<span class="state ai">AI</span>{/if}
  {#if r.error}<span class="err" title={r.error}>{r.error}</span>{/if}
</div>

<style>
  .radio {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  button {
    font-size: 12px;
    letter-spacing: 0.1em;
    font-weight: 700;
    touch-action: none;
    user-select: none;
  }
  .large button {
    font-size: 14px;
    padding: 10px 16px;
  }
  .join.on {
    color: var(--data);
    border-color: rgba(77, 250, 154, 0.6);
  }
  .talk.live {
    color: #fff;
    background: linear-gradient(180deg, #b91c1c, #7f1d1d);
    border-color: var(--crit);
    box-shadow: 0 0 14px rgba(255, 59, 59, 0.5);
  }
  .meter {
    width: 50px;
  }
  .state {
    font-family: var(--cond);
    font-size: 10px;
    letter-spacing: 0.14em;
    padding: 2px 6px;
    border-radius: 2px;
    animation: blink 1s steps(2) infinite;
  }
  .state.driver {
    color: #001a12;
    background: var(--player);
  }
  .state.ai {
    color: #001a12;
    background: var(--accent);
    animation: none;
  }
  .err {
    font-size: 11px;
    color: var(--watch);
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @keyframes blink {
    50% {
      opacity: 0.45;
    }
  }
</style>
