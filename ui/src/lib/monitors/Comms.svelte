<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { timeOfDay } from '$lib/format';
  import type { RadioEvent, Suggestion, TelemetrySnapshot } from '$lib/types';
  import RadioComposer from '$lib/components/RadioComposer.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import PushToTalk from '$lib/components/PushToTalk.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const c = $derived(snap.comms);
  let editing = $state<string | null>(null);
  let editText = $state('');
  let filter = $state<'all' | 'friend' | 'rule' | 'llm' | 'driver'>('all');
  let question = $state('');
  const QUICK_QUESTIONS = ['Radio check', 'Gap behind?', 'How are the tyres?', 'Should I box this lap?', 'Fuel?', 'Can we make it to the end on these tyres?'];
  function ask(q: string) {
    if (pitwall.ask(q)) question = '';
  }

  const log = $derived(pitwall.radioLog.filter((r) => filter === 'all' || r.source === filter));

  function startEdit(s: Suggestion) {
    editing = s.id;
    editText = s.message;
  }
  function sendEdit(s: Suggestion) {
    if (editText.trim()) pitwall.send({ type: 'suggestion.edit', id: s.id, text: editText.trim() });
    editing = null;
  }
  function remaining(s: Suggestion) {
    return Math.max(0, Math.ceil((s.expiresAtMs - pitwall.now) / 1000));
  }
  const SOURCE: Record<RadioEvent['source'], string> = { friend: 'ENGINEER', rule: 'AI RULE', llm: 'AI ANSWER', driver: 'DRIVER' };
  const OUTCOME: Record<RadioEvent['outcome'], string> = {
    spoken: 'spoken',
    queued: 'queued',
    dismissed: 'dismissed',
    expired: 'expired',
    textOnly: 'text only',
    suppressed: 'muted'
  };
</script>

{#if mode === 'tile'}
  <div class="tile">
    <div class="row">
      <span class="chip mode" class:copilot={c.mode === 'copilot'}>{c.mode === 'copilot' ? 'COPILOT APPROVAL' : 'AUTO-SPEAK'}</span>
      {#if c.muted}<span class="chip muted">AI MUTED</span>{/if}
      {#if c.friendOverrideActive}<span class="chip override num">Override {Math.ceil(c.overrideRemainingMs / 1000)}s</span>{/if}
    </div>
    <div class="pending" class:has={c.pendingSuggestions.length > 0}>
      <span class="n num">{c.pendingSuggestions.length}</span> pending suggestion{c.pendingSuggestions.length === 1 ? '' : 's'}
    </div>
    {#if c.lastDriverLine}
      <div class="last">
        <div class="label">Driver {#if c.pttActive}<span class="speaking">· TRANSMITTING</span>{/if}</div>
        <div class="line driver">“{c.lastDriverLine}”</div>
      </div>
    {/if}
    <div class="last">
      <div class="label">Engineer {#if c.speaking}<span class="speaking">· ON AIR</span>{:else if c.thinking}<span class="thinking">· THINKING</span>{/if}</div>
      <div class="line">{c.lastSpokenLine ?? '—'}</div>
    </div>
    <div class="row dim small">
      <span>PTT <b class:on={c.pttActive}>{c.pttActive ? 'LIVE' : c.micAvailable ? 'READY' : 'NO MIC'}</b></span>
      <span>STT <b class:on={c.sttAvailable}>{c.sttAvailable ? 'OK' : 'OFF'}</b></span>
      <span>LLM <b class:on={c.llmAvailable}>{c.llmAvailable ? 'OK' : 'OFF'}</b></span>
      <span>VOICE <b class:on={c.ttsAvailable}>{c.ttsAvailable ? 'OK' : 'TEXT'}</b></span>
    </div>
  </div>
{:else}
  <div class="inspect">
    <div class="row">
      <span class="label">Mode</span>
      <button class:active={c.mode === 'auto'} onclick={() => pitwall.send({ type: 'comms.setMode', mode: 'auto' })}>Auto-Speak</button>
      <button class:active={c.mode === 'copilot'} onclick={() => pitwall.send({ type: 'comms.setMode', mode: 'copilot' })}>Copilot Approval</button>
      <button class:active={c.muted} onclick={() => pitwall.send({ type: 'comms.mute', muted: !c.muted })}><Icon name={c.muted ? 'mute' : 'volume'} />{c.muted ? 'AI muted' : 'AI audible'}</button>
      {#if c.friendOverrideActive}<span class="chip override num">Friend override {Math.ceil(c.overrideRemainingMs / 1000)}s — AI P2 held</span>{/if}
    </div>

    <section>
      <div class="label">Copilot approval queue</div>
      {#if c.pendingSuggestions.length === 0}
        <p class="faint">{c.mode === 'copilot' ? 'No suggestions waiting.' : 'Switch to Copilot Approval to review AI callouts before they are spoken.'}</p>
      {/if}
      {#each c.pendingSuggestions as s (s.id)}
        <div class="card">
          <div class="card-head">
            <span class="chip">{s.priority}</span>
            <span class="chip">{s.category}</span>
            <span class="faint num">expires in {remaining(s)}s</span>
          </div>
          {#if editing === s.id}
            <textarea bind:value={editText} rows="2"></textarea>
          {:else}
            <p class="msg">{s.message}</p>
          {/if}
          {#if s.contextAttachments.length}
            <div class="att">{#each s.contextAttachments as a}<span class="chip">{a}</span>{/each}</div>
          {/if}
          <div class="actions">
            {#if editing === s.id}
              <button class="primary" onclick={() => sendEdit(s)}>Send edited</button>
              <button onclick={() => (editing = null)}>Cancel</button>
            {:else}
              <button class="primary" onclick={() => pitwall.send({ type: 'suggestion.approve', id: s.id })}><Icon name="check" /> Approve &amp; speak</button>
              <button onclick={() => startEdit(s)}><Icon name="edit" /> Edit &amp; send</button>
              <button class="danger" onclick={() => pitwall.send({ type: 'suggestion.dismiss', id: s.id })}><Icon name="close" /> Dismiss</button>
            {/if}
          </div>
        </div>
      {/each}
    </section>

    <section>
      <div class="label">Driver voice (push-to-talk)</div>
      <div class="status">
        <span class="chip" class:ok={c.micAvailable}>MIC {c.micAvailable ? 'OPEN' : 'NONE'}</span>
        <span class="chip" class:ok={c.sttAvailable}>SPEECH-TO-TEXT {c.sttAvailable ? 'READY' : 'OFFLINE'}</span>
        <span class="chip" class:ok={c.llmAvailable}>LLM {c.llmAvailable ? 'READY' : 'OFFLINE'}</span>
        <span class="chip" class:ok={c.ttsAvailable}>VOICE {c.ttsAvailable ? 'KOKORO READY' : 'TEXT ONLY'}</span>
        {#if c.thinking}<span class="chip thinking">ENGINEER THINKING…</span>{/if}
      </div>
      <div class="voice-row">
        <PushToTalk />
        <form class="ask" onsubmit={(e) => (e.preventDefault(), ask(question))}>
          <input bind:value={question} placeholder="Ask as the driver, e.g. “what's the gap behind?”" aria-label="Driver question" />
          <button class="primary" disabled={!question.trim()}>Ask engineer</button>
        </form>
      </div>
      <div class="quick">
        {#each QUICK_QUESTIONS as q}<button onclick={() => ask(q)}>{q}</button>{/each}
      </div>
      <p class="faint hint">
        In the game, map a wheel button to <b>UDP Action 1</b> (Controls) and hold it to talk. Gaps, tyres, fuel, pace and box calls are answered from telemetry
        instantly; anything else goes to the local LLM.
      </p>
    </section>

    <section>
      <div class="label">Manual radio console</div>
      <RadioComposer
        presets={['Box box, box box', 'Stay out, stay out', 'Push now, quali mode', 'Good job, keep it clean', 'Copy, understood']}
        placeholder="Type a message — Enter sends to the driver"
      />
    </section>

    <section>
      <div class="row">
        <span class="label">Radio &amp; prompt audit log</span>
        <select bind:value={filter} aria-label="Filter log">
          <option value="all">All sources</option>
          <option value="friend">Engineer</option>
          <option value="rule">AI rules</option>
          <option value="llm">AI answers</option>
          <option value="driver">Driver</option>
        </select>
      </div>
      <ul class="log">
        {#each log as r}
          <li class="p-{r.priority} o-{r.outcome}">
            <span class="faint num">{timeOfDay(r.atMs)}</span>
            <span class="src">{SOURCE[r.source]}</span>
            <span class="chip">{r.priority}</span>
            <span class="text">{r.text}</span>
            <span class="outcome">{OUTCOME[r.outcome]}</span>
            {#if r.attachments.length}<span class="faint ctx">[{r.attachments.join(' · ')}]</span>{/if}
          </li>
        {:else}
          <li class="faint">Nothing on the radio yet.</li>
        {/each}
      </ul>
    </section>
  </div>
{/if}

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .mode {
    color: var(--ok);
    border-color: var(--ok);
  }
  .mode.copilot {
    color: #c084fc;
    border-color: #c084fc;
  }
  .muted {
    color: var(--watch);
    border-color: var(--watch);
  }
  .override {
    color: #fff;
    background: #7c3aed;
    border-color: #7c3aed;
  }
  .pending {
    font-size: 13px;
    color: var(--text-dim);
  }
  .pending .n {
    font-size: 22px;
    font-weight: 600;
    color: var(--text);
  }
  .pending.has .n {
    color: #c084fc;
  }
  .line {
    font-size: 13px;
    line-height: 1.35;
  }
  .small {
    font-size: 12px;
    justify-content: space-between;
  }
  .small b {
    font-family: var(--cond);
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }
  .small b.on {
    color: var(--data);
  }
  .line.driver {
    color: var(--accent);
  }
  .thinking {
    color: var(--watch);
  }
  .status,
  .quick {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .status .ok {
    color: var(--data);
    border-color: rgba(77, 250, 154, 0.5);
  }
  .chip.thinking {
    border-color: var(--watch);
    animation: blink 1s steps(2) infinite;
  }
  .quick button {
    font-size: 12px;
  }
  .voice-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .ask {
    flex: 1;
    min-width: 260px;
    display: flex;
    gap: 6px;
  }
  .ask input {
    flex: 1;
    min-width: 0;
  }
  .hint {
    margin: 0;
    font-size: 12px;
  }
  .speaking {
    color: var(--crit);
    animation: blink 1s steps(2) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.35;
    }
  }
  .line {
    font-family: var(--mono);
    color: var(--data);
  }
  .inspect {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .active {
    border-color: var(--accent);
    color: var(--accent);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .card {
    border: 1px solid #7c3aed;
    border-radius: var(--radius);
    padding: 10px;
    background: rgba(124, 58, 237, 0.08);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .card-head,
  .att,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .msg {
    margin: 0;
    font-size: 14px;
  }
  textarea {
    width: 100%;
    resize: vertical;
  }
  .log {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 320px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
  }
  .log li {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: baseline;
    padding: 4px 6px;
    border-radius: 4px;
  }
  .log li:nth-child(odd) {
    background: rgba(255, 255, 255, 0.02);
  }
  .log .p-P0 .text {
    font-weight: 600;
  }
  .text {
    flex: 1;
    min-width: 200px;
  }
  .src {
    font-family: var(--cond);
    letter-spacing: 0.08em;
    font-size: 11px;
    color: var(--accent);
    min-width: 76px;
  }
  .outcome {
    font-size: 11px;
    color: var(--text-dim);
  }
  .o-dismissed .text,
  .o-expired .text,
  .o-suppressed .text {
    text-decoration: line-through;
    color: var(--text-faint);
  }
  .ctx {
    font-size: 11px;
    width: 100%;
    padding-left: 80px;
  }
</style>
