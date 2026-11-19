// The box's cookies, kept by the extension, never in the browser's own cookie
// jar: the box's pages can't read them, as with HttpOnly cookies on the box's
// own address. Chrome's service worker and Firefox's background page both
// use this.

import { ext } from './ext';

interface Cookie {
	value: string;
	path: string;
	expires: number | null;
}

let jar: Record<string, Cookie> | null = null;

async function cookies(): Promise<Record<string, Cookie>> {
	if (!jar) jar = ((await ext.storage.local.get('cookies')).cookies as Record<string, Cookie> | undefined) ?? {};
	const now = Date.now();
	for (const [k, c] of Object.entries(jar)) if (c.expires !== null && c.expires < now) delete jar[k];
	return jar;
}

export async function keepCookie(line: string): Promise<void> {
	const all = await cookies();
	const [pair, ...attrs] = line.split(';');
	const i = pair.indexOf('=');
	if (i < 1) return;
	const name = pair.slice(0, i).trim();
	const value = pair.slice(i + 1).trim();
	let path = '/';
	let expires: number | null = null;
	for (const a of attrs) {
		const [k, ...rest] = a.trim().split('=');
		const v = rest.join('=');
		const key = k.toLowerCase();
		if (key === 'path') path = v || '/';
		if (key === 'max-age') expires = Date.now() + Number(v) * 1000;
		if (key === 'expires' && expires === null) expires = Date.parse(v);
	}
	if (value === '' || (expires !== null && expires <= Date.now())) delete all[name];
	else all[name] = { value, path, expires };
	await ext.storage.local.set({ cookies: all });
}

export async function cookieHeader(path: string): Promise<string> {
	return Object.entries(await cookies())
		.filter(([, c]) => path.startsWith(c.path))
		.map(([n, c]) => `${n}=${c.value}`)
		.join('; ');
}
