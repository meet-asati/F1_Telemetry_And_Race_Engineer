// Duo mode team radio: listen to the radio channel (AI engineer + driver) and
// talk to the driver live from this browser. Audio runs over /ws/radio as
// 16 kHz mono Int16 frames.

import { pitwall } from './store.svelte';

const RATE = 16000;
/** Playback lead so small network hiccups don't cause gaps. */
const LEAD_S = 0.08;

type Source = 'engineer' | 'driver' | 'friend';
const SOURCES: Record<number, Source> = { 1: 'engineer', 2: 'driver', 3: 'friend' };

class TeamRadio {
  /** Connected to the radio channel and playing it. */
  on = $state(false);
  connecting = $state(false);
  /** This browser is transmitting to the driver. */
  talking = $state(false);
  /** Who was last heard on the channel (clears after a moment of silence). */
  hearing = $state<Source | null>(null);
  micLevel = $state(0);
  error = $state<string | null>(null);

  private ws: WebSocket | null = null;
  private ctx: AudioContext | null = null;
  private nextAt = 0;
  private micNode: AudioWorkletNode | null = null;
  private micStream: MediaStream | null = null;
  private hearingTimer: ReturnType<typeof setTimeout> | null = null;

  get micAllowed(): boolean {
    return typeof window !== 'undefined' && window.isSecureContext && !!navigator.mediaDevices?.getUserMedia;
  }

  /** Join the radio channel. Must run from a click (browsers gate audio). */
  async join() {
    if (this.on || this.connecting) return;
    this.connecting = true;
    this.error = null;
    try {
      this.ctx ??= new AudioContext({ latencyHint: 'interactive' });
      await this.ctx.resume();
      const proto = location.protocol === 'https:' ? 'wss' : 'ws';
      const token = pitwall.token ? `?token=${encodeURIComponent(pitwall.token)}` : '';
      const ws = new WebSocket(`${proto}://${location.host}/ws/radio${token}`);
      ws.binaryType = 'arraybuffer';
      ws.onmessage = (m) => this.onAudio(m.data as ArrayBuffer);
      ws.onclose = () => {
        this.ws = null;
        this.on = false;
        this.stopTalk();
      };
      await new Promise<void>((ok, fail) => {
        ws.onopen = () => ok();
        ws.onerror = () => fail(new Error('Radio connection refused (token?)'));
      });
      this.ws = ws;
      this.on = true;
    } catch (e) {
      this.error = (e as Error).message;
    } finally {
      this.connecting = false;
    }
  }

  leave() {
    this.stopTalk();
    this.ws?.close();
    this.ws = null;
    this.on = false;
  }

  toggle() {
    if (this.on) this.leave();
    else this.join();
  }

  private onAudio(buf: ArrayBuffer) {
    if (!this.ctx || buf.byteLength < 3) return;
    const src = SOURCES[new Uint8Array(buf, 0, 1)[0]] ?? 'engineer';
    const n = (buf.byteLength - 1) >> 1;
    const view = new DataView(buf, 1);
    const audio = this.ctx.createBuffer(1, n, RATE);
    const ch = audio.getChannelData(0);
    for (let i = 0; i < n; i++) ch[i] = view.getInt16(i * 2, true) / 32768;
    const node = this.ctx.createBufferSource();
    node.buffer = audio;
    node.connect(this.ctx.destination);
    const now = this.ctx.currentTime;
    if (this.nextAt < now + 0.01) this.nextAt = now + LEAD_S;
    node.start(this.nextAt);
    this.nextAt += audio.duration;
    this.hearing = src;
    if (this.hearingTimer) clearTimeout(this.hearingTimer);
    this.hearingTimer = setTimeout(() => (this.hearing = null), (this.nextAt - now) * 1000 + 300);
  }

  /** Key the radio and stream this browser's microphone to the driver. */
  async startTalk() {
    if (this.talking) return;
    if (!this.micAllowed) {
      this.error = 'Microphone needs HTTPS (tailscale serve) or localhost';
      return;
    }
    if (!this.on) await this.join();
    if (!this.ws || !this.ctx) return;
    try {
      if (!this.micNode) {
        await this.ctx.audioWorklet.addModule('/radio-worklet.js');
        this.micStream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 } });
        const srcNode = this.ctx.createMediaStreamSource(this.micStream);
        this.micNode = new AudioWorkletNode(this.ctx, 'radio-capture');
        this.micNode.port.onmessage = (e) => {
          const frame = e.data as Int16Array;
          let peak = 0;
          for (let i = 0; i < frame.length; i += 8) peak = Math.max(peak, Math.abs(frame[i]));
          this.micLevel = Math.max(peak / 32768, this.micLevel * 0.8);
          if (this.talking && this.ws?.readyState === WebSocket.OPEN) this.ws.send(frame.buffer);
        };
        srcNode.connect(this.micNode);
        // Keep the worklet in the rendered graph (silently), or it may not run.
        const mute = this.ctx.createGain();
        mute.gain.value = 0;
        this.micNode.connect(mute).connect(this.ctx.destination);
      }
      this.ws.send(JSON.stringify({ type: 'tx', active: true }));
      this.talking = true;
    } catch (e) {
      this.error = `Microphone: ${(e as Error).message}`;
    }
  }

  stopTalk() {
    if (!this.talking) return;
    this.talking = false;
    this.micLevel = 0;
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify({ type: 'tx', active: false }));
  }
}

export const teamRadio = new TeamRadio();
