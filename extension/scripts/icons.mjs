// Renders the ZOOMRIYA mark (from Box Desktop's logo) to the PNG sizes
// browsers ask for, through headless Chromium. Run once when the mark changes:
//   node scripts/icons.mjs PATH/TO/mark.svg
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { dirname } from 'node:path';
// The mark's own images, inlined, so the page needs nothing else.
const svg = readFileSync(process.argv[2], 'utf8').replace(/href="([^"]+\.png)"/g, (_, f) =>
	`href="data:image/png;base64,${readFileSync(join(dirname(process.argv[2]), f)).toString('base64')}"`
);
const chrome = `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-linux64/chrome-headless-shell`;
const proc = spawn(chrome, ['--headless', '--no-sandbox', `--user-data-dir=${mkdtempSync(join(tmpdir(), 'zr-icons-'))}`, '--remote-debugging-port=19250', 'about:blank'], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let ws;
for (let i = 0; i < 50 && !ws; i++) {
	await sleep(100);
	try {
		const page = (await (await fetch('http://127.0.0.1:19250/json')).json()).find((t) => t.type === 'page');
		if (page) ws = new WebSocket(page.webSocketDebuggerUrl);
	} catch {}
}
await new Promise((r) => ws.addEventListener('open', r));
let id = 0;
const wait = new Map();
ws.addEventListener('message', (e) => {
	const m = JSON.parse(e.data);
	if (m.id && wait.has(m.id)) { wait.get(m.id)(m); wait.delete(m.id); }
});
const send = (method, params = {}) => new Promise((r) => { const i = ++id; wait.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
mkdirSync('public/zr/icons', { recursive: true });
for (const px of [16, 32, 48, 128]) {
	await send('Emulation.setDeviceMetricsOverride', { width: px, height: px, deviceScaleFactor: 1, mobile: false });
	const html = `<html><body style="margin:0;background:transparent">${svg.replace('<svg', `<svg width="${px}" height="${px}"`)}</body></html>`;
	await send('Page.navigate', { url: 'data:text/html;base64,' + Buffer.from(html).toString('base64') });
	await sleep(300);
	await send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } });
	const r = await send('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: px, height: px, scale: 1 } });
	writeFileSync(`public/zr/icons/${px}.png`, Buffer.from(r.result.data, 'base64'));
	console.log('icon', px);
}
proc.kill();
process.exit(0);
