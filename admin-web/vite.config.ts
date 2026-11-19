import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

// Built to dist/, which the servers serve from their [admin] web_dir
// (/usr/share/zoomriya-connect/admin). `npm run dev` passes the API on to a
// server's GUI address on this machine.
export default defineConfig({
	plugins: [svelte()],
	build: { outDir: 'dist', emptyOutDir: true, assetsInlineLimit: 0, target: 'es2022' },
	server: { proxy: { '/admin/api': { target: 'http://127.0.0.1:8444' } } },
	test: { environment: 'node', include: ['src/**/*.test.ts'] }
});
