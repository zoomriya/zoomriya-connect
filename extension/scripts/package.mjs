// Writes each browser's package from the one build in dist/unpacked: a folder
// to load for testing, and a zip for its store (docs/laptops.md, "Browsers").
//
// | Package | Browsers | Store | How the box's pages run |
// |---|---|---|---|
// | chromium | Chrome, Brave, Vivaldi | Chrome Web Store | The extension's own address, answered by its service worker (src/sw.ts), WebRTC in an offscreen page |
// | edge | Microsoft Edge | Edge Add-ons | The same |
// | opera | Opera | Opera add-ons | The same |
// | firefox | Firefox | Firefox Add-ons | Box Desktop's own address, answered by the box's service worker, WebRTC in the background page (src/firefox/background.ts) |
//
// Firefox's package is Manifest V2: its V3 background pages stop when idle,
// which would drop the WebRTC connection, and Firefox keeps V2 supported.
//
// ZOOMRIYA_EXTENSION_DEV=1: may reach a box on this computer without asking,
// so automated tests can pair (headless browsers can't answer a prompt).
import { cpSync, mkdirSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

const version = process.env.ZOOMRIYA_EXTENSION_VERSION ?? '0.1.0';
const dev = process.env.ZOOMRIYA_EXTENSION_DEV === '1';
const name = dev ? 'ZOOMRIYA (development)' : 'ZOOMRIYA';
const description = 'Your ZOOMRIYA Box from anywhere, end to end encrypted.';
const icons = { 16: 'zr/icons/16.png', 32: 'zr/icons/32.png', 48: 'zr/icons/48.png', 128: 'zr/icons/128.png' };
const action = {
	default_title: 'ZOOMRIYA',
	default_popup: 'zr/popup.html',
	default_icon: { 16: 'zr/icons/16.png', 32: 'zr/icons/32.png' }
};
const csp = "script-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";
// The box's address is asked for at pairing, for that box alone.
const boxes = ['https://*/*', 'http://127.0.0.1/*', 'http://localhost/*'];
const devHosts = ['http://127.0.0.1/*', 'http://localhost/*'];

const chromium = {
	manifest_version: 3,
	name,
	short_name: 'ZOOMRIYA',
	version,
	description,
	minimum_chrome_version: '137',
	icons,
	action,
	background: { service_worker: 'zr/sw.js', type: 'module' },
	// Only what it needs: its own storage, and the offscreen page WebRTC needs.
	permissions: ['storage', 'offscreen'],
	optional_host_permissions: boxes,
	content_security_policy: { extension_pages: csp },
	...(dev ? { host_permissions: devHosts } : {})
};

const firefox = {
	manifest_version: 2,
	name,
	short_name: 'ZOOMRIYA',
	version,
	description,
	icons,
	browser_action: action,
	background: { page: 'zr/background.html', persistent: true },
	// Its own storage; webRequest to check the box's certificate on Box
	// Desktop's address (blocking, so a wrong one never loads).
	permissions: ['storage', 'webRequest', 'webRequestBlocking', ...(dev ? devHosts : [])],
	optional_permissions: boxes,
	content_security_policy: csp,
	browser_specific_settings: { gecko: { id: 'extension@zoomriya.com', strict_min_version: '140.0' } }
};

/** What each package leaves out: the other browser's files. */
const chromiumOnly = ['zr/sw.js', 'zr/offscreen.html', 'zr/offscreen.js', 'zr/open.html', 'zr/open.js'];
const firefoxOnly = ['zr/background.html', 'zr/background.js'];

const packages = [
	{ name: 'chromium', manifest: chromium, drop: firefoxOnly },
	{ name: 'edge', manifest: chromium, drop: firefoxOnly },
	{ name: 'opera', manifest: chromium, drop: firefoxOnly },
	{ name: 'firefox', manifest: firefox, drop: chromiumOnly }
];

rmSync('dist/packages', { recursive: true, force: true });
mkdirSync('dist/packages', { recursive: true });
for (const p of packages) {
	const dir = `dist/${p.name}`;
	rmSync(dir, { recursive: true, force: true });
	cpSync('dist/unpacked', dir, { recursive: true });
	for (const f of p.drop) if (existsSync(`${dir}/${f}`)) rmSync(`${dir}/${f}`);
	writeFileSync(`${dir}/manifest.json`, JSON.stringify(p.manifest, null, 2) + '\n');
	const zip = `${process.cwd()}/dist/packages/zoomriya-${p.name}-${version}.zip`;
	execFileSync('python3', ['-m', 'zipfile', '-c', zip, 'manifest.json', 'zr'], { cwd: dir });
	console.log(`${p.name}: dist/${p.name}/ and dist/packages/zoomriya-${p.name}-${version}.zip`);
}
