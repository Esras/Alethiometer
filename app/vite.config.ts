import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed port and fails if it's taken.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
    // Browser mode: the dev server, from the UI's own origin (it sends no CORS headers).
    proxy: { '/call': { target: process.env.ALETHIOMETER_DEVSERVER ?? 'http://127.0.0.1:1421', changeOrigin: true } },
  },
  build: { target: 'safari16', outDir: 'dist', emptyOutDir: true },
});
