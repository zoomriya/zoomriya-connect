// The laptop's keys, made by the browser and never exportable: the device
// key (ECDSA P-256), which the box pins, and the link key (Ed25519), which
// passes name. Kept in IndexedDB as CryptoKey objects: the browser keeps the
// private halves to itself.

import { b64url, concat, fromB64url, fromZ32, utf8, z32 } from './codec';

/** What every signed message starts with (zoomriya-keys' `framed`). */
const DOMAIN = utf8('zoomriya-connect\0');

export type Purpose = 'pass/v1' | 'request/v1' | 'laptop-offer/v1' | 'laptop-answer/v1' | 'laptop-paired/v1';

/** The exact bytes signed for `purpose`. */
export function framed(purpose: Purpose, msg: Uint8Array): Uint8Array<ArrayBuffer> {
	return concat(DOMAIN, utf8(purpose), new Uint8Array([0]), msg);
}

/** What the device key signs to vouch for a link key. */
export function linkKeyMessage(link: Uint8Array): Uint8Array<ArrayBuffer> {
	return concat(utf8('zoomriya-connect\0link-key/v1\0'), link);
}

export interface Keys {
	device: CryptoKeyPair;
	link: CryptoKeyPair;
}

const DB = 'zoomriya';
const STORE = 'keys';

function db(): Promise<IDBDatabase> {
	return new Promise((ok, no) => {
		const r = indexedDB.open(DB, 1);
		r.onupgradeneeded = () => r.result.createObjectStore(STORE);
		r.onsuccess = () => ok(r.result);
		r.onerror = () => no(r.error);
	});
}

async function get<T>(key: string): Promise<T | undefined> {
	const d = await db();
	return new Promise((ok, no) => {
		const r = d.transaction(STORE).objectStore(STORE).get(key);
		r.onsuccess = () => ok(r.result as T | undefined);
		r.onerror = () => no(r.error);
	});
}

async function put(key: string, value: unknown): Promise<void> {
	const d = await db();
	return new Promise((ok, no) => {
		const t = d.transaction(STORE, 'readwrite');
		t.objectStore(STORE).put(value, key);
		t.oncomplete = () => ok();
		t.onerror = () => no(t.error);
	});
}

/** The laptop's keys, if it has made them. */
export async function loadKeys(): Promise<Keys | undefined> {
	return get<Keys>('keys');
}

/** Makes new keys, replacing any old ones: pairing again starts afresh. */
export async function newKeys(): Promise<Keys> {
	const device = (await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, [
		'sign',
		'verify'
	])) as CryptoKeyPair;
	const link = (await crypto.subtle.generateKey({ name: 'Ed25519' }, false, [
		'sign',
		'verify'
	])) as CryptoKeyPair;
	const keys = { device, link };
	await put('keys', keys);
	return keys;
}

/** Forgets the keys: this laptop has to pair again. */
export async function forgetKeys(): Promise<void> {
	const d = await db();
	await new Promise<void>((ok, no) => {
		const t = d.transaction(STORE, 'readwrite');
		t.objectStore(STORE).clear();
		t.oncomplete = () => ok();
		t.onerror = () => no(t.error);
	});
}

/** The device key's public half: SEC1, base64url, as the box pins it. */
export async function devicePublic(k: Keys): Promise<string> {
	return b64url(new Uint8Array(await crypto.subtle.exportKey('raw', k.device.publicKey)));
}

/** The link key's public half, as the box writes keys. */
export async function linkPublic(k: Keys): Promise<string> {
	return z32(new Uint8Array(await crypto.subtle.exportKey('raw', k.link.publicKey)));
}

/** Signs with the device key: the raw 64 bytes, base64url. */
export async function signDevice(k: Keys, msg: Uint8Array<ArrayBuffer>): Promise<string> {
	const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, k.device.privateKey, msg);
	return b64url(new Uint8Array(sig));
}

/** The device key's signature over the link key. */
export async function linkSig(k: Keys): Promise<string> {
	const link = new Uint8Array(await crypto.subtle.exportKey('raw', k.link.publicKey));
	return signDevice(k, linkKeyMessage(link));
}

/**
 * Opens a note the box sealed (`<body>.<signature>`, base64url), checking
 * the box key's signature for `purpose`. Returns the body, or null.
 */
export async function openSealed<T>(sealed: string, purpose: Purpose, boxKey: string): Promise<T | null> {
	const [body, sig] = sealed.split('.');
	if (!body || !sig) return null;
	const raw = fromZ32(boxKey, 32);
	if (!raw) return null;
	const key = await crypto.subtle.importKey('raw', raw, { name: 'Ed25519' }, false, ['verify']);
	const json = fromB64url(body);
	const ok = await crypto.subtle.verify({ name: 'Ed25519' }, key, fromB64url(sig), framed(purpose, json));
	if (!ok) return null;
	return JSON.parse(new TextDecoder().decode(json)) as T;
}

/** The body of a sealed note, unchecked: only to learn which key to check it with. */
export function peekSealed<T>(sealed: string): T | null {
	try {
		return JSON.parse(new TextDecoder().decode(fromB64url(sealed.split('.')[0]))) as T;
	} catch {
		return null;
	}
}

/**
 * Signs a request to a relay with the link key, as the relay checks it
 * (zoomriya-keys' `sign_request`): the `zoomriya-signature` header's value.
 */
export async function signRequest(k: Keys, method: string, path: string, body: Uint8Array<ArrayBuffer>): Promise<string> {
	const time = Math.floor(Date.now() / 1000);
	const hash = [...new Uint8Array(await crypto.subtle.digest('SHA-256', body))]
		.map((b) => b.toString(16).padStart(2, '0'))
		.join('');
	const msg = utf8(`${method.toUpperCase()}\n${path}\n${time}\n${hash}`);
	const sig = await crypto.subtle.sign({ name: 'Ed25519' }, k.link.privateKey, framed('request/v1', msg));
	return `${await linkPublic(k)}.${time}.${b64url(new Uint8Array(sig))}`;
}

/** SHA-256, base64url. */
export async function sha256(text: string): Promise<string> {
	return b64url(new Uint8Array(await crypto.subtle.digest('SHA-256', utf8(text))));
}
