<script lang="ts">
  // Top-down F1 car in wireframe line art. Every part can be tinted (damage,
  // wear or temperature); untinted parts use the phosphor-green screen ink.

  type Parts = Partial<Record<'frontLeftWing' | 'frontRightWing' | 'rearWing' | 'floor' | 'diffuser' | 'sidepods', string | null>>;

  let {
    parts = {},
    wheels = [null, null, null, null],
    wheelFill = [null, null, null, null],
    height = 240,
    label = 'Car diagram'
  }: {
    parts?: Parts;
    /** Stroke colour per wheel, FL FR RL RR. */
    wheels?: (string | null)[];
    /** Fill colour per wheel (e.g. wear), FL FR RL RR. */
    wheelFill?: (string | null)[];
    height?: number;
    label?: string;
  } = $props();

  const INK = 'var(--data)';
  const ink = (c?: string | null) => c ?? INK;
  const tint = (c?: string | null) => (c ? `color-mix(in srgb, ${c} 28%, transparent)` : 'rgba(77, 250, 154, 0.05)');
  /** Inline style (presentation attributes do not resolve CSS variables everywhere). */
  const st = (c?: string | null) => `stroke:${ink(c)};fill:${tint(c)}`;
  // FL, FR, RL, RR wheel boxes.
  const W = [
    { x: 20, y: 62, w: 34, h: 58 },
    { x: 146, y: 62, w: 34, h: 58 },
    { x: 16, y: 276, w: 40, h: 68 },
    { x: 144, y: 276, w: 40, h: 68 }
  ];
</script>

<svg viewBox="0 0 200 400" height={height} width={height / 2} role="img" aria-label={label}>
  <g fill="none" stroke-width="1.4" stroke-linejoin="round">
    <!-- suspension arms -->
    <g style="stroke:{INK}" opacity="0.55">
      <path d="M54 80 L90 92 M54 104 L90 112 M146 80 L110 92 M146 104 L110 112" />
      <path d="M56 296 L80 286 M56 324 L82 316 M144 296 L120 286 M144 324 L118 316" />
    </g>

    <!-- floor (dashed outline under the bodywork) -->
    <path
      d="M64 150 L52 176 L48 268 L64 312 L136 312 L152 268 L148 176 L136 150 Z"
      style={st(parts.floor)}
      stroke-dasharray={parts.floor ? undefined : '4 3'}
    />

    <!-- front wing: two halves + endplates -->
    <path d="M18 24 L98 24 L98 40 L18 40 Z M18 18 L18 44" style={st(parts.frontLeftWing)} />
    <path d="M102 24 L182 24 L182 40 L102 40 Z M182 18 L182 44" style={st(parts.frontRightWing)} />

    <!-- nose + chassis + engine cover -->
    <path d="M94 14 L106 14 L112 110 L122 150 L124 210 L118 300 L104 330 L96 330 L82 300 L76 210 L78 150 L88 110 Z" style={st()} />
    <!-- cockpit + halo -->
    <ellipse cx="100" cy="168" rx="11" ry="20" style="stroke:{INK}" />
    <path d="M86 150 Q100 136 114 150 M100 140 L100 150" style="stroke:{INK}" opacity="0.7" />
    <!-- airbox / spine -->
    <path d="M100 192 L100 296" style="stroke:{INK}" opacity="0.4" />

    <!-- sidepods -->
    <path d="M78 168 L58 178 L54 250 L70 292 L80 292 Z" style={st(parts.sidepods)} />
    <path d="M122 168 L142 178 L146 250 L130 292 L120 292 Z" style={st(parts.sidepods)} />

    <!-- diffuser -->
    <path d="M70 330 L130 330 L136 352 L64 352 Z M80 330 L78 352 M100 330 L100 352 M120 330 L122 352" style={st(parts.diffuser)} />

    <!-- rear wing -->
    <path d="M36 358 L164 358 L164 378 L36 378 Z M36 352 L36 384 M164 352 L164 384 M100 352 L100 358" style={st(parts.rearWing)} />

    <!-- wheels -->
    {#each W as b, i}
      <rect x={b.x} y={b.y} width={b.w} height={b.h} rx="7" stroke-width="2" style="stroke:{ink(wheels[i])};fill:{wheelFill[i] ? tint(wheelFill[i]) : 'rgba(2,7,10,0.8)'}" />
      <path d="M{b.x + 6} {b.y + b.h / 2} H{b.x + b.w - 6}" style="stroke:{ink(wheels[i])}" opacity="0.5" />
    {/each}
  </g>
</svg>

<style>
  svg {
    display: block;
    filter: drop-shadow(0 0 3px rgba(77, 250, 154, 0.35));
    max-height: 100%;
  }
</style>
