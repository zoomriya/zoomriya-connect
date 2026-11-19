// What goes to the box with a request of its pages, and what of the box's
// answer goes back to the page. The same in Chrome and Firefox.

import type { Box } from './box';
import { cookieHeader, keepCookie } from './cookies';

/** Headers never sent on: the page's own address, and cookies, which come from the jar. */
const NOT_SENT = new Set(['cookie', 'referer', 'origin']);
/** Headers never given to the page. */
const NOT_SHOWN = new Set(['set-cookie', 'content-length', 'content-encoding', 'transfer-encoding', 'connection', 'keep-alive']);

/**
 * The request's headers as the box gets them. `sameOrigin`: the request came
 * from one of the box's own pages (Chrome: always; Firefox: the box's
 * service worker says so). connectd then sets the box's own Origin.
 */
export async function outgoing(method: string, path: string, headers: [string, string][], sameOrigin: boolean): Promise<[string, string][]> {
	const out = headers.filter(([k]) => !NOT_SENT.has(k.toLowerCase()));
	const change = !['GET', 'HEAD'].includes(method);
	if (sameOrigin && (change || headers.some(([k]) => k.toLowerCase() === 'origin'))) out.push(['origin', 'same']);
	const c = await cookieHeader(path.split('?')[0]);
	if (c) out.push(['cookie', c]);
	return out;
}

/**
 * The answer's headers as the page gets them: cookies go to the jar, and a
 * redirect to the box's own name stays on the page's address.
 */
export async function incoming(headers: [string, string][], box: Box): Promise<[string, string][]> {
	const out: [string, string][] = [];
	for (const [k, v] of headers) {
		if (k === 'set-cookie') {
			await keepCookie(v);
			continue;
		}
		if (NOT_SHOWN.has(k)) continue;
		if (k === 'location') {
			try {
				const to = new URL(v, `https://${box.fqdn}`);
				if (to.hostname === box.fqdn) {
					out.push([k, to.pathname + to.search + to.hash]);
					continue;
				}
			} catch {
				/* left as it was */
			}
		}
		out.push([k, v]);
	}
	return out;
}
