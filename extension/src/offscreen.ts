// Chrome's offscreen page: WebRTC, which Chrome lets an extension use only in
// a page like this one, never in its service worker. The service worker sends
// each request of the box's pages here (src/sw.ts) over a MessageChannel; the
// connection itself is src/lib/conn.ts.

import { handle, setOnUpdate, type Req } from './lib/conn';

// Offscreen pages can't use the extension's storage: the service worker keeps
// what each answer brings.
setOnUpdate((boxKey, update) => {
	void navigator.serviceWorker.ready.then((r) => r.active?.postMessage({ t: 'box-update', boxKey, update }));
});

navigator.serviceWorker.addEventListener('message', (e) => {
	const port = e.ports[0];
	if (e.data?.t === 'request' && port) void handle(e.data as Req, port);
	if (e.data?.t === 'ping' && port) port.postMessage({ t: 'pong' });
});
// Tell the service worker this page is ready for requests.
navigator.serviceWorker.ready.then((r) => r.active?.postMessage({ t: 'offscreen-ready' }));
