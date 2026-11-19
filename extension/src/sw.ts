/// <reference lib="webworker" />
// The extension's service worker. Every request for the extension's own
// address, except its own files under /zr/, is one of the box's pages asking
// the box: it goes over WebRTC (src/offscreen.ts), and the answer streams
// back. The box's pages run from this address, so nothing in between can read
// or change them (docs/laptops.md).
//
// The box's cookies are kept by the extension (src/lib/cookies.ts), never in
// the browser's cookie jar.

import { loadBox, saveBox, updated, type BoxUpdate } from './lib/box';
import { incoming, outgoing } from './lib/headers';

declare const self: ServiceWorkerGlobalScope;

self.addEventListener('install', () => void self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));

// --- The offscreen page --------------------------------------------------------

let ready: (() => void) | null = null;
let readyPromise: Promise<void> | null = null;

self.addEventListener('message', (e) => {
	if (e.data?.t === 'offscreen-ready' && ready) {
		ready();
		ready = null;
	}
	// A fresh pass and the relays now, from the box's latest answer. Only for
	// the box this laptop is paired with: the offscreen page checked the
	// answer against the pinned box key.
	if (e.data?.t === 'box-update') {
		const { boxKey, update } = e.data as { boxKey: string; update: BoxUpdate };
		e.waitUntil(
			loadBox().then(async (box) => {
				if (box && box.boxKey === boxKey) await saveBox(updated(box, update));
			})
		);
	}
});

async function offscreen(): Promise<Client> {
	const find = async () =>
		(await self.clients.matchAll({ includeUncontrolled: true, type: 'all' })).find((c) =>
			c.url.endsWith('/zr/offscreen.html')
		);
	const found = await find();
	if (found) return found;
	if (!readyPromise) {
		readyPromise = new Promise<void>((ok) => (ready = ok));
		const contexts = await chrome.runtime.getContexts({ contextTypes: [chrome.runtime.ContextType.OFFSCREEN_DOCUMENT] });
		if (contexts.length === 0) {
			await chrome.offscreen.createDocument({
				url: 'zr/offscreen.html',
				reasons: [chrome.offscreen.Reason.WEB_RTC],
				justification: 'The encrypted connection to the family’s ZOOMRIYA Box.'
			});
		}
		await Promise.race([readyPromise, new Promise((ok) => setTimeout(ok, 3000))]);
		readyPromise = null;
	}
	for (let i = 0; i < 20; i++) {
		const c = await find();
		if (c) return c;
		await new Promise((ok) => setTimeout(ok, 100));
	}
	throw new Error("The extension's connection page didn't start.");
}

// --- Pages ---------------------------------------------------------------------

function page(title: string, text: string, status: number): Response {
	const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
	const html = `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)}</title><link rel="stylesheet" href="/zr/notice.css">
<main><h1>${esc(title)}</h1><p>${esc(text)}</p><p><a href="">Try again</a></p></main>`;
	return new Response(html, { status, headers: { 'content-type': 'text/html; charset=utf-8' } });
}

// --- Requests --------------------------------------------------------------------

self.addEventListener('fetch', (event) => {
	const url = new URL(event.request.url);
	if (url.origin !== self.location.origin || url.pathname.startsWith('/zr/')) return;
	event.respondWith(toBox(event.request, url));
});

/** What the offscreen page sends back about one request. */
type Msg =
	| { t: 'response'; status: number; headers: [string, string][] }
	| { t: 'chunk'; bytes: ArrayBuffer }
	| { t: 'end' }
	| { t: 'error'; message: string };

async function toBox(req: Request, url: URL): Promise<Response> {
	const box = await loadBox();
	if (!box) return page('Pair this laptop', 'Click the ZOOMRIYA button in the toolbar, then pair this laptop with your box at home.', 409);

	const path = url.pathname + url.search;
	// Only the extension's own pages reach this address: always the same origin.
	const headers = await outgoing(req.method, path, [...req.headers], true);
	const body = ['GET', 'HEAD'].includes(req.method) ? null : await req.arrayBuffer();

	let client: Client;
	try {
		client = await offscreen();
	} catch (e) {
		return page("Can't reach your box", (e as Error).message, 502);
	}
	const channel = new MessageChannel();
	const port = channel.port1;
	// Every message is kept from the start: the body may arrive while the
	// headers are still being handled (saving cookies takes a moment).
	const inbox: Msg[] = [];
	let wake: (() => void) | null = null;
	port.onmessage = (e) => {
		inbox.push(e.data as Msg);
		wake?.();
	};
	const next = async (): Promise<Msg> => {
		while (inbox.length === 0) await new Promise<void>((ok) => (wake = ok));
		wake = null;
		return inbox.shift() as Msg;
	};
	client.postMessage({ t: 'request', method: req.method, path, headers, body, box }, [channel.port2, ...(body ? [body] : [])]);
	const head = await next();
	if (head.t !== 'response') {
		port.close();
		return req.mode === 'navigate'
			? page("Can't reach your box", head.t === 'error' ? head.message : '', 502)
			: new Response(JSON.stringify({ code: 'unreachable', message: head.t === 'error' ? head.message : '' }), {
					status: 502,
					headers: { 'content-type': 'application/json' }
				});
	}

	const out = new Headers(await incoming(head.headers, box));
	const status = head.status as number;
	const empty = status === 204 || status === 205 || status === 304 || req.method === 'HEAD';
	if (empty) {
		port.close();
		return new Response(null, { status, headers: out });
	}
	const stream = new ReadableStream<Uint8Array>({
		async pull(ctl) {
			const m = await next();
			if (m.t === 'chunk') ctl.enqueue(new Uint8Array(m.bytes));
			else if (m.t === 'end') {
				ctl.close();
				port.close();
			} else if (m.t === 'error') {
				ctl.error(new Error(m.message));
				port.close();
			}
		},
		cancel() {
			port.postMessage({ t: 'cancel' });
			port.close();
		}
	});
	return new Response(stream, { status, headers: out });
}
