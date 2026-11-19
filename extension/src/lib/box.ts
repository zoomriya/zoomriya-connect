// The box this laptop is paired with, as the extension keeps it.

import { ext } from './ext';

export interface Box {
	/** The box's name, such as maplehouse.zoomriya.app. */
	fqdn: string;
	/** The box key, pinned at pairing (z-base-32). */
	boxKey: string;
	/** This laptop's ID on the box. */
	device: string;
	/** The pass, for the relay. */
	pass: string;
	until: number;
	/** The ZOOMRIYA Connect server the box uses. */
	server: string;
	/** Where the box answers on the home network: https://<fqdn>:7445. */
	home: string;
	/** The laptop's own name on the box. */
	name: string;
	/** The relays, for connecting from away. */
	relays?: string[];
	/** STUN servers, so WebRTC learns this laptop's public address. */
	ice?: IceServer[];
	/** Box Desktop's address, such as https://maplehouse.zoomriya.app:7443: Firefox opens the box here. */
	desktop?: string;
	/** The keys of the box's certificate (SHA-256 of its SubjectPublicKeyInfo, base64), which Firefox checks. */
	tlsKeys?: string[];
}

export interface IceServer {
	urls: string[];
	username?: string;
	credential?: string;
}

/** What each answer from the box brings: a fresh pass, the relays now. */
export interface BoxUpdate {
	pass?: string;
	until?: number;
	relays?: string[];
	ice?: IceServer[];
	tls_keys?: string[];
}

/** The box with what an answer brought; unchanged where it brought nothing. */
export function updated(box: Box, u: BoxUpdate): Box {
	return {
		...box,
		...(u.pass ? { pass: u.pass, until: u.until ?? box.until } : {}),
		...(u.relays?.length ? { relays: u.relays } : {}),
		...(u.ice ? { ice: u.ice } : {}),
		...(u.tls_keys?.length ? { tlsKeys: u.tls_keys } : {})
	};
}

export async function loadBox(): Promise<Box | null> {
	const got = await ext.storage.local.get('box');
	return (got.box as Box | undefined) ?? null;
}

export async function saveBox(box: Box): Promise<void> {
	await ext.storage.local.set({ box });
}

/**
 * The host permission for a box address: every port of its name, as browsers
 * match addresses by name.
 */
export function permissionFor(address: string): string {
	const u = new URL(address);
	return `${u.protocol}//${u.hostname}/*`;
}

/** Whether this is the Firefox build: no service worker of its own. */
export function isFirefox(): boolean {
	return !(ext.runtime.getManifest() as { background?: { service_worker?: string } }).background?.service_worker;
}

export async function forgetBox(): Promise<void> {
	await ext.storage.local.remove(['box', 'cookies']);
}

/**
 * The home endpoint's address for what the person typed: a box name
 * (maplehouse.zoomriya.app, or home.smith.ca), or a full address for
 * development (http://127.0.0.1:7445).
 */
export function homeFor(typed: string): string | null {
	const t = typed.trim().replace(/\/+$/, '');
	if (/^https?:\/\//.test(t)) {
		try {
			return new URL(t).origin;
		} catch {
			return null;
		}
	}
	const name = t.toLowerCase();
	if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$/.test(name)) return null;
	return `https://${name}:7445`;
}
