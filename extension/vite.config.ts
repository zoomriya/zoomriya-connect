import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

// Everything the extension itself owns is under /zr/: every other path of the
// extension's address belongs to the box (src/sw.ts). scripts/package.mjs then
// writes each browser's manifest and zip from dist/.
export default defineConfig({
	plugins: [svelte()],
	build: {
		outDir: 'dist/unpacked',
		emptyOutDir: true,
		assetsInlineLimit: 0,
		target: 'es2022',
		modulePreload: false,
		rollupOptions: {
			input: {
				popup: 'zr/popup.html',
				offscreen: 'zr/offscreen.html',
				open: 'zr/open.html',
				sw: 'src/sw.ts',
				// Firefox only (src/firefox/background.ts).
				background: 'zr/background.html'
			},
			output: {
				entryFileNames: 'zr/[name].js',
				chunkFileNames: 'zr/chunks/[name]-[hash].js',
				assetFileNames: 'zr/assets/[name]-[hash][extname]'
			}
		}
	},
	test: { environment: 'node', include: ['src/**/*.test.ts'] }
});
