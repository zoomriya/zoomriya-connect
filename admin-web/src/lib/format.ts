// Numbers and times, in the GUI's language.

import type { Lang } from './words';

/** "3 minutes ago", "yesterday", or a date, from seconds since 1970. */
export function ago(seconds: number, now: number, lang: Lang): string {
	const d = Math.max(0, now - seconds);
	const rtf = new Intl.RelativeTimeFormat(lang, { numeric: 'auto' });
	if (d < 60) return rtf.format(0, 'second');
	if (d < 3600) return rtf.format(-Math.floor(d / 60), 'minute');
	if (d < 86400) return rtf.format(-Math.floor(d / 3600), 'hour');
	if (d < 7 * 86400) return rtf.format(-Math.floor(d / 86400), 'day');
	return new Intl.DateTimeFormat(lang, { dateStyle: 'medium' }).format(new Date(seconds * 1000));
}

/** A whole number with the language's grouping: 12,345 or 12 345. */
export function count(n: number, lang: Lang): string {
	return new Intl.NumberFormat(lang).format(n);
}

/** The first and last few characters of a key. */
export function shortKey(key: string): string {
	return key.length <= 12 ? key : `${key.slice(0, 6)}…${key.slice(-4)}`;
}
