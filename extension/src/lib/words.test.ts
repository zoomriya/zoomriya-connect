import { describe, expect, it } from 'vitest';
import { pickLang, words } from './words';

describe('words', () => {
	it('has every word in both languages', () => {
		const en = Object.keys(words.en).sort();
		expect(Object.keys(words.fr).sort()).toEqual(en);
	});
	it('never names what it must not', () => {
		const all = JSON.stringify(words);
		expect(all).not.toMatch(/docker|debian|trixie/i);
		expect(all).not.toMatch(/Zoomriya/);
	});
	it('picks a language', () => {
		expect(pickLang(['fr-CA'])).toBe('fr');
		expect(pickLang(['de'])).toBe('en');
	});
});
