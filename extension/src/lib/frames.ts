// The data channel's frames (zoomriya-proto's laptop.rs): JSON text frames
// for requests and answers, binary frames for bodies.

export const CHANNEL = 'zoomriya';
export const MAX_CHUNK = 16 * 1024;

export type Frame =
	| { t: 'request'; id: number; method: string; path: string; headers: [string, string][] }
	| { t: 'response'; id: number; status: number; headers: [string, string][] }
	| { t: 'cancel'; id: number }
	| { t: 'error'; id: number; message: string };

/** A body part: request ID, 0, bytes. */
export function chunk(id: number, bytes: Uint8Array): Uint8Array {
	const out = new Uint8Array(5 + bytes.length);
	new DataView(out.buffer).setUint32(0, id);
	out[4] = 0;
	out.set(bytes, 5);
	return out;
}

/** The end of a body: request ID, 1. */
export function end(id: number): Uint8Array {
	const out = new Uint8Array(5);
	new DataView(out.buffer).setUint32(0, id);
	out[4] = 1;
	return out;
}

export type Data = { id: number; end: false; bytes: Uint8Array } | { id: number; end: true };

/** Reads a binary frame. */
export function readData(buf: ArrayBuffer): Data | null {
	if (buf.byteLength < 5) return null;
	const v = new DataView(buf);
	const id = v.getUint32(0);
	const kind = v.getUint8(4);
	if (kind === 0) return { id, end: false, bytes: new Uint8Array(buf, 5) };
	if (kind === 1 && buf.byteLength === 5) return { id, end: true };
	return null;
}
