// The connection to the box: WebRTC, set up at home or through a relay, and
// the box's pages' requests sent over it (docs/laptops.md). Chrome runs this
// in its offscreen page (src/offscreen.ts), Firefox in its background page
// (src/firefox/background.ts). Each request comes with a MessagePort that
// gets the answer: {t:'response'}, then {t:'chunk'}s, then {t:'end'}, or
// {t:'error'}. The port may send {t:'cancel'}.

import type { Box, BoxUpdate, IceServer } from './box';
import { CHANNEL, MAX_CHUNK, chunk, end, readData, type Frame } from './frames';
import { framed, linkPublic, loadKeys, openSealed, sha256, signDevice, signRequest, type Keys } from './keys';
import { utf8 } from './codec';

interface Waiting {
	port: MessagePort;
	started: boolean;
}

interface Conn {
	pc: RTCPeerConnection;
	dc: RTCDataChannel;
	open: Promise<void>;
	nextId: number;
	waiting: Map<number, Waiting>;
}

let conn: Conn | null = null;

/** Where a fresh pass and the relays now go: each browser keeps them its own way. */
let onUpdate: (boxKey: string, update: BoxUpdate) => void = () => {};
export function setOnUpdate(f: (boxKey: string, update: BoxUpdate) => void) {
	onUpdate = f;
}

/** Why the box can't be reached, in words for the page. */
class Unreachable extends Error {
	/** Newer ICE servers the box sent, worth one more try with. */
	fresh?: IceServer[];
}

async function connect(box: Box): Promise<Conn> {
	const keys = await loadKeys();
	if (!keys) throw new Unreachable('This laptop has no keys. Pair it again at home.');
	const pc = new RTCPeerConnection({ iceServers: box.ice ?? [] });
	const dc = pc.createDataChannel(CHANNEL, { ordered: true });
	dc.binaryType = 'arraybuffer';
	const c: Conn = { pc, dc, open: Promise.resolve(), nextId: 1, waiting: new Map() };
	c.open = new Promise<void>((ok, no) => {
		const timer = setTimeout(() => no(new Unreachable("The box didn't answer.")), 20_000);
		dc.onopen = () => {
			clearTimeout(timer);
			ok();
		};
		pc.onconnectionstatechange = () => {
			if (pc.connectionState === 'failed') {
				clearTimeout(timer);
				no(new Unreachable("Couldn't connect to the box."));
			}
		};
	});
	dc.onmessage = (e) => receive(c, e.data);
	dc.onclose = () => drop(c, 'The connection to the box closed.');
	pc.onconnectionstatechange = ((prev) => () => {
		prev?.call(pc, new Event('connectionstatechange'));
		if (pc.connectionState === 'failed' || pc.connectionState === 'closed') drop(c, 'The connection to the box was lost.');
	})(pc.onconnectionstatechange);

	await pc.setLocalDescription(await pc.createOffer());
	await gathered(pc);
	const sdp = pc.localDescription?.sdp ?? '';
	const link = await linkPublic(keys);
	const time = Math.floor(Date.now() / 1000);
	const sig = await signDevice(keys, framed('laptop-offer/v1', utf8(`${link}\n${time}\n${sdp}`)));
	let body: { answer?: string };
	try {
		body = await ask(box, keys, JSON.stringify({ link, time, sdp, sig }));
	} catch (e) {
		pc.close();
		throw e;
	}
	// The answer must be sealed by the box this laptop paired with.
	const answer = await openSealed<{ offer: string; sdp: string } & BoxUpdate>(body.answer ?? '', 'laptop-answer/v1', box.boxKey);
	if (!answer || answer.offer !== (await sha256(sdp))) {
		pc.close();
		throw new Unreachable("The answer didn't come from your box. Nothing was sent.");
	}
	// A fresh pass and the relays now, for the service worker to keep.
	const update: BoxUpdate = { pass: answer.pass, until: answer.until, relays: answer.relays, ice: answer.ice, tls_keys: answer.tls_keys };
	onUpdate(box.boxKey, update);
	await pc.setRemoteDescription({ type: 'answer', sdp: answer.sdp });
	try {
		await c.open;
	} catch (e) {
		pc.close();
		// The TURN credentials kept from last time may have run out: the
		// answer brought new ones, so the caller tries once more with them.
		const fresh = answer.ice ?? [];
		if (e instanceof Unreachable && fresh.length && JSON.stringify(fresh) !== JSON.stringify(box.ice ?? [])) {
			e.fresh = fresh;
		}
		throw e;
	}
	return c;
}

/** The relays' path for an offer from away (zoomriya-proto's RELAY_CONNECT). */
const RELAY_CONNECT = '/zoomriya/v1/laptop/connect';

/**
 * Sends the signed offer to the box: at home straight to it, away through a
 * relay with this laptop's pass. Returns the box's reply, `{answer}`.
 */
async function ask(box: Box, keys: Keys, offer: string): Promise<{ answer?: string }> {
	// At home: the box's own address answers only on its network, so give it
	// a moment and no more.
	try {
		const res = await fetch(`${box.home}/laptop/connect`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: offer,
			signal: AbortSignal.timeout(3000)
		});
		const got = await res.json().catch(() => ({}));
		if (res.ok) return got;
		// The box itself said no, for a reason other than "not at home".
		if (got.code !== 'not_home') throw new Unreachable(got.message ?? "The box didn't answer.");
	} catch (e) {
		if (e instanceof Unreachable) throw e;
	}
	// Away: through the relays, in order.
	const relays = box.relays ?? [];
	let why = relays.length
		? "Can't reach your box. Check this laptop is online."
		: "Can't reach your box. At home, check this laptop is on the box's network.";
	const bytes = new TextEncoder().encode(offer) as Uint8Array<ArrayBuffer>;
	for (const relay of relays) {
		try {
			const res = await fetch(relay.replace(/\/+$/, '') + RELAY_CONNECT, {
				method: 'POST',
				headers: {
					'content-type': 'application/json',
					authorization: `Bearer ${box.pass}`,
					'zoomriya-signature': await signRequest(keys, 'POST', RELAY_CONNECT, bytes)
				},
				body: bytes,
				signal: AbortSignal.timeout(25_000)
			});
			const got = await res.json().catch(() => ({}));
			if (res.ok) return got;
			why = got.message ?? why;
			// Refused (the pass, or the box said no): another relay would say the same.
			if (res.status === 403 || res.status === 401) break;
		} catch {
			/* the next relay */
		}
	}
	throw new Unreachable(why);
}

/** Waits for ICE to find this laptop's addresses, or two seconds. */
function gathered(pc: RTCPeerConnection): Promise<void> {
	if (pc.iceGatheringState === 'complete') return Promise.resolve();
	return new Promise((ok) => {
		const done = () => {
			if (pc.iceGatheringState === 'complete') ok();
		};
		pc.addEventListener('icegatheringstatechange', done);
		setTimeout(ok, 2000);
	});
}

function drop(c: Conn, why: string) {
	if (conn === c) conn = null;
	for (const w of c.waiting.values()) w.port.postMessage({ t: 'error', message: why });
	c.waiting.clear();
	try {
		c.pc.close();
	} catch {
		/* already closed */
	}
}

function receive(c: Conn, data: string | ArrayBuffer) {
	if (typeof data === 'string') {
		let f: Frame;
		try {
			f = JSON.parse(data) as Frame;
		} catch {
			return;
		}
		const w = c.waiting.get(f.id);
		if (!w) return;
		if (f.t === 'response') {
			w.started = true;
			w.port.postMessage({ t: 'response', status: f.status, headers: f.headers });
		} else if (f.t === 'error') {
			c.waiting.delete(f.id);
			w.port.postMessage({ t: 'error', message: f.message });
		}
		return;
	}
	const d = readData(data);
	if (!d) return;
	const w = c.waiting.get(d.id);
	if (!w) return;
	if (d.end) {
		c.waiting.delete(d.id);
		w.port.postMessage({ t: 'end' });
	} else {
		const copy = d.bytes.slice().buffer;
		w.port.postMessage({ t: 'chunk', bytes: copy }, [copy]);
	}
}

let connecting: Promise<Conn> | null = null;

// The box comes with each request: Chrome's offscreen page can't read the
// extension's storage itself.
async function ready(box: Box): Promise<Conn> {
	if (conn && conn.dc.readyState === 'open') return conn;
	if (!connecting) {
		connecting = (async () => {
			let c: Conn;
			try {
				c = await connect(box);
			} catch (e) {
				if (!(e instanceof Unreachable) || !e.fresh) throw e;
				c = await connect({ ...box, ice: e.fresh });
			}
			conn = c;
			return c;
		})().finally(() => {
			connecting = null;
		});
	}
	return connecting;
}

/** Sends, waiting while the channel's buffer is full. */
async function send(dc: RTCDataChannel, data: string | Uint8Array) {
	if (dc.bufferedAmount > 1 << 20) {
		await new Promise<void>((ok) => {
			dc.bufferedAmountLowThreshold = 256 << 10;
			dc.addEventListener('bufferedamountlow', () => ok(), { once: true });
		});
	}
	if (typeof data === 'string') dc.send(data);
	else dc.send(data as Uint8Array<ArrayBuffer>);
}

export interface Req {
	t: 'request';
	method: string;
	path: string;
	headers: [string, string][];
	body: ArrayBuffer | null;
	box: Box;
}

export async function handle(req: Req, port: MessagePort) {
	let c: Conn;
	try {
		c = await ready(req.box);
	} catch (e) {
		port.postMessage({ t: 'error', message: (e as Error).message });
		return;
	}
	const id = c.nextId++;
	c.waiting.set(id, { port, started: false });
	port.onmessage = (e) => {
		if (e.data?.t === 'cancel' && c.waiting.delete(id)) {
			void send(c.dc, JSON.stringify({ t: 'cancel', id } satisfies Frame));
		}
	};
	await send(c.dc, JSON.stringify({ t: 'request', id, method: req.method, path: req.path, headers: req.headers } satisfies Frame));
	if (req.body) {
		const bytes = new Uint8Array(req.body);
		for (let at = 0; at < bytes.length; at += MAX_CHUNK) await send(c.dc, chunk(id, bytes.subarray(at, at + MAX_CHUNK)));
	}
	await send(c.dc, end(id));
}

