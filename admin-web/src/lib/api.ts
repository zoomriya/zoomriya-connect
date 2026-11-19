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
}

export interface RelayOverview {
	kind: 'relay';
	version: string;
	mode: 'enrolled' | 'control_plane';
	boxes: number;
	connected: { boxes: number; devices: number };
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
}

export interface Report {
	id: number;
	name: string;
	reason: string;
	at: number;
}
