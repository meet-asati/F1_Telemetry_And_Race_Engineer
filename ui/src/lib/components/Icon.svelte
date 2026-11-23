<script lang="ts" module>
  export type IconName =
    | 'popout'
    | 'close'
    | 'settings'
    | 'lock'
    | 'mic'
    | 'volume'
    | 'mute'
    | 'check'
    | 'edit'
    | 'flag'
    | 'radio'
    | 'sun'
    | 'cloud'
    | 'partcloud'
    | 'rain'
    | 'storm';

  // 24×24 stroke paths; circles are encoded as "c:cx,cy,r".
  const PATHS: Record<IconName, string[]> = {
    popout: ['M14 4h6v6', 'M20 4l-9 9', 'M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5'],
    close: ['M6 6l12 12', 'M18 6L6 18'],
    settings: ['M4 7h9', 'M17 7h3', 'M4 17h3', 'M11 17h9', 'c:15,7,2', 'c:9,17,2'],
    lock: ['M6 11h12v9H6z', 'M8.5 11V8a3.5 3.5 0 0 1 7 0v3'],
    mic: ['M9 6a3 3 0 0 1 6 0v5a3 3 0 0 1-6 0z', 'M5.5 11a6.5 6.5 0 0 0 13 0', 'M12 17.5V21'],
    volume: ['M4 9.5h3.5L12 6v12l-4.5-3.5H4z', 'M15.5 9a4 4 0 0 1 0 6', 'M18 6.5a7.5 7.5 0 0 1 0 11'],
    mute: ['M4 9.5h3.5L12 6v12l-4.5-3.5H4z', 'M16 9.5l5 5', 'M21 9.5l-5 5'],
    check: ['M5 12.5l4.5 4.5L19 7'],
    edit: ['M4 20h4L19 9l-4-4L4 16z', 'M13 7l4 4'],
    flag: ['M6 21V4', 'M6 4h11l-2.5 4L17 12H6'],
    radio: ['M4 10h16v10H4z', 'M7 10l9-6', 'c:9,15,2', 'M14 14h3', 'M14 17h3'],
    sun: ['c:12,12,4', 'M12 2.5v2.5', 'M12 19v2.5', 'M2.5 12H5', 'M19 12h2.5', 'M5.3 5.3l1.8 1.8', 'M16.9 16.9l1.8 1.8', 'M5.3 18.7l1.8-1.8', 'M16.9 7.1l1.8-1.8'],
    partcloud: ['M8 9.5a4 4 0 0 1 7.4-2', 'M8 3v1.5', 'M3.5 8.5H5', 'M4.6 5l1 1', 'M8 19h9a3.5 3.5 0 0 0 .4-7 5 5 0 0 0-9.6-1A4 4 0 0 0 8 19z'],
    cloud: ['M7 18h10a4 4 0 0 0 .5-8 6 6 0 0 0-11.4-1.2A4.6 4.6 0 0 0 7 18z'],
    rain: ['M7 15h10a4 4 0 0 0 .5-8 6 6 0 0 0-11.4-1.2A4.6 4.6 0 0 0 7 15z', 'M8.5 18l-1 3', 'M12.5 18l-1 3', 'M16.5 18l-1 3'],
    storm: ['M7 15h10a4 4 0 0 0 .5-8 6 6 0 0 0-11.4-1.2A4.6 4.6 0 0 0 7 15z', 'M12.5 15l-2 3.5h3l-2 3.5']
  };
</script>

<script lang="ts">
  let { name, size = 14, title = undefined }: { name: IconName; size?: number; title?: string } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  role={title ? 'img' : undefined}
  aria-hidden={title ? undefined : 'true'}
  aria-label={title}
>
  {#each PATHS[name] as d}
    {#if d.startsWith('c:')}
      {@const [cx, cy, r] = d.slice(2).split(',').map(Number)}
      <circle {cx} {cy} {r} />
    {:else}
      <path {d} />
    {/if}
  {/each}
</svg>

<style>
  .icon {
    flex-shrink: 0;
    display: inline-block;
    vertical-align: -0.15em;
  }
</style>
