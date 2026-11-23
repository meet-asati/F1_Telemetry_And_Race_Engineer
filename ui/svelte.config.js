import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
export default {
  preprocess: vitePreprocess(),
  kit: {
    // SPA: the Rust daemon serves index.html for every unknown path.
    adapter: adapter({ fallback: 'index.html', pages: 'build', assets: 'build' })
  }
};
