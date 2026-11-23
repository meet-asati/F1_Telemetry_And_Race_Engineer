<script lang="ts">
  // Rolling 30 s trace of the player's throttle, brake and speed, drawn every frame.
  import { onMount } from 'svelte';
  import { pitwall } from '$lib/store.svelte';

  let { windowMs = 30000, showSpeed = true }: { windowMs?: number; showSpeed?: boolean } = $props();

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;

  function draw() {
    const dpr = window.devicePixelRatio || 1;
    const w = wrap.clientWidth;
    const h = wrap.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const padL = 30;
    const padT = 4;
    const padB = 4;
    const ih = h - padT - padB;
    const iw = w - padL - 4;
    const y = (pct: number) => padT + (1 - pct / 100) * ih;

    // Grid + axis labels.
    ctx.font = '9px Consolas, ui-monospace, monospace';
    ctx.textAlign = 'right';
    for (const g of [0, 25, 50, 75, 100]) {
      ctx.strokeStyle = g === 0 || g === 100 ? 'rgba(110,255,200,0.16)' : 'rgba(110,255,200,0.07)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(padL, Math.round(y(g)) + 0.5);
      ctx.lineTo(w - 4, Math.round(y(g)) + 0.5);
      ctx.stroke();
      ctx.fillStyle = '#4b675d';
      ctx.fillText(`${g}%`, padL - 4, y(g) + 3);
    }

    const tr = pitwall.trace;
    if (tr.length < 2) return;
    const now = performance.now();
    const x = (t: number) => padL + (1 - (now - t) / windowMs) * iw;
    const line = (pick: (s: (typeof tr)[number]) => number, color: string, width: number, fill = false) => {
      ctx.save();
      ctx.beginPath();
      let started = false;
      let firstX = 0;
      let lastX = 0;
      for (const s of tr) {
        if (now - s.t > windowMs) continue;
        const px = x(s.t);
        const py = y(pick(s));
        if (!started) {
          ctx.moveTo(px, py);
          firstX = px;
          started = true;
        } else ctx.lineTo(px, py);
        lastX = px;
      }
      if (!started) {
        ctx.restore();
        return;
      }
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.shadowColor = color;
      ctx.shadowBlur = 6;
      ctx.stroke();
      if (fill) {
        ctx.shadowBlur = 0;
        ctx.lineTo(lastX, y(0));
        ctx.lineTo(firstX, y(0));
        ctx.closePath();
        const g = ctx.createLinearGradient(0, padT, 0, h);
        g.addColorStop(0, color.replace('1)', '0.22)'));
        g.addColorStop(1, color.replace('1)', '0)'));
        ctx.fillStyle = g;
        ctx.fill();
      }
      ctx.restore();
    };
    if (showSpeed) line((s) => Math.min(100, (s.speed / 350) * 100), 'rgba(34,211,238,1)', 1.2);
    line((s) => s.throttle, 'rgba(77,250,154,1)', 1.6, true);
    line((s) => s.brake, 'rgba(255,59,59,1)', 1.6, true);
  }

  onMount(() => {
    let raf = 0;
    const loop = () => {
      draw();
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  });
</script>

<div class="wrap" bind:this={wrap}>
  <canvas bind:this={canvas} aria-label="Throttle, brake and speed trace"></canvas>
</div>

<style>
  .wrap {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 50px;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
