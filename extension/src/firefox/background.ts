// Firefox's background page (docs/laptops.md, "Firefox").
//
// Firefox gives extensions no service worker of their own, so the box's pages
// open at Box Desktop's real address, where the box's own service worker
// (zr-sw.js, served by Box Desktop) answers them. It hands each request to a
// hidden frame of the box's zr-bridge.html kept here, which passes it to this
// page; this page sends it to the box over WebRTC (src/lib/conn.ts), exactly
// as Chrome's extension does, and streams the answer back.
//
// | Part | Here |
// |---|---|
// | The WebRTC connection | src/lib/conn.ts, the same as Chrome's |
// | The bridge frame | Opened when the laptop is paired, again every minute until it answers |
// | The box's certificate | Every page of the box's name must show the key the box sealed (`tlsKeys`); anything else is blocked |
// | Pairing's last step | The popup asks `setup`: the frame installs the box's service worker while the laptop is at home |

import { ext } from '../lib/ext';
import { loadBox, saveBox, updated, type Box } from '../lib/box';
import { handle, setOnUpdate } from '../lib/conn';
import { incoming, outgoing } from '../lib/headers';

// Firefox's own API, for what Chrome has no equivalent of.
declare const browser: {
	webRequest: {
		onHeadersReceived: {
			addListener(
				f: (d: { requestId: string; url: string }) => Promise<{ cancel?: boolean }> | { cancel?: boolean },
				filter: { urls: string[] },
				extra: string[]
			): void;
		};
		getSecurityInfo(
			requestId: string,
			options: object
		): Promise<{ state: string; certificates: { subjectPublicKeyInfoDigest: { sha256: string } }[] }>;
	};
};

let box: Box | null = null;

setOnUpdate(async (boxKey, update) => {
	const b = await loadBox();
	if (b && b.boxKey === boxKey) await saveBox(updated(b, update));
});

// --- The bridge ------------------------------------------------------------------

let frame: HTMLIFrameElement | null = null;
let frameOrigin = '';
let bridgeReady = false;
let waiting: (() => void)[] = [];

function openBridge() {
	const desktop = box?.desktop;
	const origin = desktop ? new URL(desktop).origin : '';
	if (frame && origin !== frameOrigin) closeBridge();
	if (!origin || frame) return;
	frameOrigin = origin;
	frame = document.createElement('iframe');
	frame.src = `${origin}/zr-bridge.html`;
	document.documentElement.appendChild(frame);
}

function closeBridge() {
	frame?.remove();
	frame = null;
	bridgeReady = false;
}

// Until it answers: after the laptop comes home, say, or a network change.
setInterval(() => {
	if (!bridgeReady) {
		closeBridge();
		openBridge();
	}
}, 60_000);

window.addEventListener('message', (e) => {
	if (!frame || e.source !== frame.contentWindow || e.origin !== frameOrigin) return;
	const m = e.data;
	if (m?.t === 'bridge-ready') {
		bridgeReady = true;
		for (const f of waiting.splice(0)) f();
	} else if (m?.t === 'request' && e.ports[0]) {
		void serve(m, e.ports[0]);
	}
});

/** Waits for the bridge, up to `ms`. */
function bridge(ms: number): Promise<boolean> {
	if (bridgeReady) return Promise.resolve(true);
	return new Promise((ok) => {
		const done = () => ok(bridgeReady);
		waiting.push(done);
		setTimeout(() => {
			waiting = waiting.filter((f) => f !== done);
			ok(bridgeReady);
		}, ms);
	});
}

interface FromBridge {
	method: string;
	path: string;
	headers: [string, string][];
	body: ArrayBuffer | null;
	sameOrigin?: boolean;
}

/** One request of the box's pages: to the box over WebRTC, and back. */
async function serve(m: FromBridge, port: MessagePort) {
	const b = await loadBox();
	if (!b) {
		port.postMessage({ t: 'error', message: 'This laptop isn’t paired. Click the ZOOMRIYA button in the toolbar.' });
		return;
	}
	const headers = await outgoing(m.method, m.path, m.headers, m.sameOrigin !== false);
	const ch = new MessageChannel();
	// In order: the answer's headers take a moment (cookies are saved first).
	let chain = Promise.resolve();
	ch.port1.onmessage = (ev) => {
		const a = ev.data;
		chain = chain.then(async () => {
			if (a.t === 'response') port.postMessage({ t: 'response', status: a.status, headers: await incoming(a.headers, b) });
			else if (a.t === 'chunk') port.postMessage(a, [a.bytes]);
			else port.postMessage(a);
		});
	};
	port.onmessage = (ev) => {
		if (ev.data?.t === 'cancel') ch.port1.postMessage({ t: 'cancel' });
	};
	void handle({ t: 'request', method: m.method, path: m.path, headers, body: m.body ?? null, box: b }, ch.port2);
}

// --- The box's certificate -----------------------------------------------------------

// Every answer from Box Desktop's address over HTTPS: its certificate's key
// must be one the box sealed. Stops anyone else with a certificate for the name from
// serving the box's service worker or pages.
browser.webRequest.onHeadersReceived.addListener(
	async (d) => {
		const keys = box?.tlsKeys ?? [];
		if (!box?.desktop || keys.length === 0) return {};
		// Box Desktop's address only. The home endpoint's answers are sealed by
		// the box key, so a new certificate never stops reconnecting at home.
		const u = new URL(d.url);
		if (u.protocol !== 'https:' || u.origin !== new URL(box.desktop).origin) return {};
		try {
			const info = await browser.webRequest.getSecurityInfo(d.requestId, {});
			const key = info.certificates?.[0]?.subjectPublicKeyInfoDigest?.sha256;
			if (info.state === 'secure' && key && keys.includes(key)) return {};
		} catch {
			/* blocked below */
		}
		console.warn('ZOOMRIYA: blocked a page of your box’s name with another certificate');
		return { cancel: true };
	},
	{ urls: ['https://*/*'] },
	['blocking']
);

// --- The popup ----------------------------------------------------------------------

ext.runtime.onMessage.addListener((msg, _sender, reply) => {
	if (msg?.t === 'setup') {
		openBridge();
		void bridge(msg.wait ?? 15_000).then((ready) => reply({ ready }));
		return true;
	}
	return false;
});

// --- Start ---------------------------------------------------------------------------

async function refresh() {
	box = await loadBox();
	if (!box) closeBridge();
	else openBridge();
}
ext.storage.onChanged.addListener((changes) => {
	if (changes.box) void refresh();
});
void refresh();
