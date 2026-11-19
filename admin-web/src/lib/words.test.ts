import { describe, expect, it } from 'vitest';
import { fill, pickLang, words } from './words';

describe('words', () => {
	it('has every word in both languages', () => {
		const en = Object.keys(words.en).sort();
		expect(Object.keys(words.fr).sort()).toEqual(en);
		for (const k of en) expect((words.fr as Record<string, string>)[k]).not.toBe('');
	});
	it('fills placeholders', () => {
		expect(fill(words.en.added, { n: 3 })).toBe('3 new tokens added.');
		expect(fill('{a} {b}', { a: 1 })).toBe('1 {b}');
	});
	it('picks a language', () => {
		expect(pickLang(['fr-CA', 'en'])).toBe('fr');
		expect(pickLang(['de-DE', 'en-GB'])).toBe('en');
		expect(pickLang(['de'])).toBe('en');
	});
	it('never names what it must not', () => {
		const all = JSON.stringify(words);
		expect(all).not.toMatch(/docker|debian|trixie/i);
		expect(all).not.toMatch(/Zoomriya/);
	});
});
