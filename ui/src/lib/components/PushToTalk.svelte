<script lang="ts">
  // Hold-to-talk button: keys the driver's radio through the pit-wall PC's
  // microphone (the same path as the wheel button mapped to "UDP Action N").
  import { pitwall } from '$lib/store.svelte';
  import Icon from './Icon.svelte';

  let { big = false }: { big?: boolean } = $props();

  let held = $state(false);
  const c = $derived(pitwall.snap?.comms);
  const onAir = $derived(held || !!c?.pttActive);

  function down(e: PointerEvent) {
    if (held) return;
    held = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    pitwall.ptt(true);
  }
  function up() {
    if (!held) return;
    held = false;
    pitwall.ptt(false);
  }
  function key(e: KeyboardEvent) {
    if (e.key !== ' ' && e.key !== 'Enter') return;
    e.preventDefault();
    if (e.type === 'keydown' && !e.repeat && !held) {
      held = true;
      pitwall.ptt(true);
    } else if (e.type === 'keyup') up();
  }
</script>

<svelte:window onblur={up} />

<button
  class="ptt"
  class:big
  class:on={onAir}
  disabled={c ? !c.micAvailable : true}
  title={c?.micAvailable ? 'Hold to talk to the engineer' : 'No microphone open on the pit-wall PC'}
  onpointerdown={down}
  onpointerup={up}
  onpointercancel={up}
  onlostpointercapture={up}
  onkeydown={key}
  onkeyup={key}
  oncontextmenu={(e) => e.preventDefault()}
>
  <Icon name="mic" size={big ? 34 : 16} />
  <span>{onAir ? 'ON AIR' : c?.thinking ? 'ENGINEER…' : 'HOLD TO TALK'}</span>
</button>

<style>
  .ptt {
    justify-content: center;
    font-family: var(--cond);
    font-weight: 700;
    letter-spacing: 0.16em;
    border-color: var(--hairline-strong);
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }
  .ptt.big {
    width: 100%;
    padding: 22px;
    font-size: 26px;
    gap: 14px;
    border-radius: 12px;
  }
  .ptt.on {
    color: #fff;
    background: linear-gradient(180deg, #b91c1c, #7f1d1d);
    border-color: var(--crit);
    box-shadow: 0 0 24px rgba(255, 59, 59, 0.45);
  }
</style>
