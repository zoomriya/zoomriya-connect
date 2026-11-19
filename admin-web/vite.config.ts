import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

// Two pages from one source:
// - the GUI, built to dist/, which the servers serve from their [admin]
//   web_dir (/usr/share/zoomriya-connect/admin);
// - with `--mode account`, the families' account page, built to
//   dist-account/, which the control plane serves at /account/ from its
//   [accounts] page_dir (/usr/share/zoomriya-connect/account).
// `npm run dev` passes the API on to a server's GUI address on this machine.
export default defineConfig(({ mode }) =>
	mode === 'account'
		? {
				plugins: [svelte()],
				root: 'account',
				base: '/account/',
				build: { outDir: '../dist-account', emptyOutDir: true, assetsInlineLimit: 0, target: 'es2022' }
			}
		: {
				plugins: [svelte()],
				build: { outDir: 'dist', emptyOutDir: true, assetsInlineLimit: 0, target: 'es2022' },
				server: { proxy: { '/admin/api': { target: 'http://127.0.0.1:8444' } } },
				test: { environment: 'node', include: ['src/**/*.test.ts'] }
			}
);
