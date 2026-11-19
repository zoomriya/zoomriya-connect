// Encodings the box uses: z-base-32 for keys, base64url for signatures and
// sealed notes.

const Z32 = 'ybndrfg8ejkmcpqxot1uwisza345h769';

/** Bytes as z-base-32, as iroh and the box write keys. */
export function z32(bytes: Uint8Array): string {
	let out = '';
	let acc = 0;
	let bits = 0;
	for (const b of bytes) {
		acc = (acc << 8) | b;
		bits += 8;
		while (bits >= 5) {
			out += Z32[(acc >>> (bits - 5)) & 31];
			bits -= 5;
		}
		acc &= (1 << bits) - 1;
	}
	if (bits > 0) out += Z32[(acc << (5 - bits)) & 31];
	return out;
}

/** z-base-32 back to bytes; `null` if it isn't. */
export function fromZ32(s: string, length: number): Uint8Array<ArrayBuffer> | null {
	const out = new Uint8Array(length);
	let acc = 0;
	let bits = 0;
	let i = 0;
	for (const c of s) {
		const v = Z32.indexOf(c);
		if (v < 0) return null;
		acc = (acc << 5) | v;
		bits += 5;
		if (bits >= 8) {
			if (i >= length) return null;
			out[i++] = (acc >>> (bits - 8)) & 255;
			bits -= 8;
			acc &= (1 << bits) - 1;
		}
	}
	return i === length ? out : null;
}

/** Bytes as base64url, without padding. */
export function b64url(bytes: Uint8Array): string {
	let s = '';
	for (const b of bytes) s += String.fromCharCode(b);
	return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** base64url back to bytes. */
export function fromB64url(s: string): Uint8Array<ArrayBuffer> {
	const std = s.replace(/-/g, '+').replace(/_/g, '/');
	const bin = atob(std + '='.repeat((4 - (std.length % 4)) % 4));
	const out = new Uint8Array(bin.length);
	for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
	return out;
}

export const utf8 = (s: string): Uint8Array<ArrayBuffer> => new TextEncoder().encode(s) as Uint8Array<ArrayBuffer>;

export function concat(...parts: Uint8Array[]): Uint8Array<ArrayBuffer> {
	const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
	let at = 0;
	for (const p of parts) {
		out.set(p, at);
		at += p.length;
	}
	return out;
}
