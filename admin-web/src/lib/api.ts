// The admin API. The sign-in token lives in this tab's session storage only,
// and goes with every request as a bearer token: there's no cookie for a page
// elsewhere to ride on.

const KEY = 'zoomriya-connect-admin-token';

export class SignedOut extends Error {}

export function token(): string | null {
	try {
		return sessionStorage.getItem(KEY);
	} catch {
		return null;
	}
}

export function setToken(t: string | null): void {
	try {
		if (t) sessionStorage.setItem(KEY, t);
		else sessionStorage.removeItem(KEY);
	} catch {
		/* private window: the token lasts until the page is closed */
	}
	memory = t;
}

let memory: string | null = null;

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
	const t = token() ?? memory;
	if (!t) throw new SignedOut();
	const res = await fetch(`/admin/api${path}`, {
		method,
		headers: {
			authorization: `Bearer ${t}`,
			...(body === undefined ? {} : { 'content-type': 'application/json' })
		},
		body: body === undefined ? undefined : JSON.stringify(body)
	});
	if (res.status === 401) {
		setToken(null);
		throw new SignedOut();
	}
	if (!res.ok) throw new Error((await res.text()).trim() || res.statusText);
	if (res.status === 204) return undefined as T;
	return (await res.json()) as T;
}

export const get = <T>(path: string) => call<T>('GET', path);
export const post = <T>(path: string, body?: unknown) => call<T>('POST', path, body ?? {});
export const put = <T>(path: string, body: unknown) => call<T>('PUT', path, body);

/** Checks a token by asking for the overview with it. */
export async function signIn(t: string): Promise<Overview> {
	setToken(t.trim());
	return get<Overview>('/overview');
}

export interface RelayHealth {
	url: string;
	up: boolean;
	ms: number | null;
	checked: number;
}

export interface ControlPlaneOverview {
	kind: 'control_plane';
	version: string;
	zone: string;
	boxes: { all: number; factory: number; stolen: number; off: number };
	new: { day: number; week: number };
	reports_open: number;
	factory_tokens: { unused: number; used: number };
	setup_messages: { boxes: number; guests: number };
	guest_relay: boolean;
	relays: RelayHealth[];
	connected: { boxes: number; devices: number };
	relay_use: { month: string; bytes: number };
	allowances: Allowances;
	accounts: { all: number; blocked: number };
}

/** A family's ZOOMRIYA Connect account. */
export interface Account {
	username: string;
	blocked: boolean;
	max_boxes: number;
	boxes: string[];
	created_at: number;
	/** Locked after wrong passwords until then (0: not locked). */
	locked_until: number;
}

/** The monthly relay allowance. 0 GB means no limit. */
export interface Allowances {
	default_gb: number;
	factory_gb: number;
	/** How fast a box past its allowance may go, in kilobytes a second. */
	slow_kbps: number;
}

export interface ConnectedBox {
	name: string | null;
	key: string;
	relay: string;
	devices: number;
}

export interface RelayOverview {
	kind: 'relay';
	version: string;
	mode: 'enrolled' | 'control_plane';
	boxes: number;
	connected: { boxes: number; devices: number };
	connected_boxes: { key: string; devices: number }[];
	slowed: number;
	since_start: {
		boxes_allowed: number;
		devices_allowed: number;
		refused: number;
		refused_removed: number;
		refused_expired: number;
		dropped: number;
	};
}

export type Overview = ControlPlaneOverview | RelayOverview;

export interface BoxSummary {
	name: string;
	key: string;
	factory: boolean;
	stolen: boolean;
	disabled: boolean;
	registered_at: number;
	door_until: number;
	has_record: boolean;
	/** Bytes through the relays this month. */
	used: number;
	/** Its own allowance in GB, if set (0: no limit). */
	own_allowance_gb: number | null;
	/** The allowance that applies, in GB (0: no limit). */
	allowance_gb: number;
	slowed: boolean;
	/** The relay it's connected to now. */
	relay: string | null;
	devices: number;
	/** The ZOOMRIYA Connect account it's linked to. */
	account: string | null;
}

export interface Report {
	id: number;
	name: string;
	reason: string;
	at: number;
}
