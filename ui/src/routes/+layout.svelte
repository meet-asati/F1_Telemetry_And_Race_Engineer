<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { pitwall } from '$lib/store.svelte';

  let { children } = $props();
  onMount(() => pitwall.start());
</script>

{@render children()}
