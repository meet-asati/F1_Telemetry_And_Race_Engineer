<script lang="ts">
  import { pitwall } from '$lib/store.svelte';
  import { timeOfDay } from '$lib/format';
  import type { RadioEvent, Suggestion, TelemetrySnapshot } from '$lib/types';
  import RadioComposer from '$lib/components/RadioComposer.svelte';

  let { snap, mode }: { snap: TelemetrySnapshot; mode: 'tile' | 'inspect' } = $props();

  const c = $derived(snap.comms);
  let editing = $state<string | null>(null);
  let editText = $state('');
  let filter = $state<'all' | 'friend' | 'rule' | 'llm' | 'driver'>('all');

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
  const SOURCE: Record<RadioEvent['source'], string> = { friend: '👤 Engineer', rule: '🤖 AI rule', llm: '🧠 AI answer', driver: '🎧 Driver' };
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
    <div class="last">
      <div class="label">Last on radio {c.speaking ? '· 🔊 speaking' : ''}</div>
      <div class="line">{c.lastSpokenLine ?? '—'}</div>
    </div>
    <div class="row dim small">
      <span>PTT {c.pttActive ? '🔴 live' : 'idle'}</span>
      <span>TTS {c.ttsAvailable ? '✓ ready' : '✗ text only'}</span>
    </div>
  </div>
{:else}
  <div class="inspect">
    <div class="row">
      <span class="label">Mode</span>
      <button class:active={c.mode === 'auto'} onclick={() => pitwall.send({ type: 'comms.setMode', mode: 'auto' })}>Auto-Speak</button>
      <button class:active={c.mode === 'copilot'} onclick={() => pitwall.send({ type: 'comms.setMode', mode: 'copilot' })}>Copilot Approval</button>
      <button class:active={c.muted} onclick={() => pitwall.send({ type: 'comms.mute', muted: !c.muted })}>{c.muted ? '🔇 AI muted' : '🔊 AI audible'}</button>
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
              <button class="primary" onclick={() => pitwall.send({ type: 'suggestion.approve', id: s.id })}>✓ Approve &amp; speak</button>
              <button onclick={() => startEdit(s)}>✎ Edit &amp; send</button>
              <button class="danger" onclick={() => pitwall.send({ type: 'suggestion.dismiss', id: s.id })}>✕ Dismiss</button>
            {/if}
          </div>
        </div>
      {/each}
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
    color: var(--text-dim);
    min-width: 92px;
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
