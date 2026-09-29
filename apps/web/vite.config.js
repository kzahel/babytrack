import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  server: {
    fs: { allow: ['../..'] },
    proxy: { '/v1': process.env.BABYTRACK_RELAY_URL || 'http://127.0.0.1:4877' },
  },
});
