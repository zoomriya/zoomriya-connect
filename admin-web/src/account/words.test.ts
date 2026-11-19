import { describe, expect, it } from 'vitest';
import { accountWords } from './words';

describe('account page words', () => {
	it('has every word in both languages', () => {
		const en = Object.keys(accountWords.en).sort();
		expect(Object.keys(accountWords.fr).sort()).toEqual(en);
		for (const k of en) expect((accountWords.fr as Record<string, string>)[k]).not.toBe('');
	});
	it('never names what it must not', () => {
		const all = JSON.stringify(accountWords);
		expect(all).not.toMatch(/docker|debian|trixie/i);
		expect(all).not.toMatch(/Zoomriya/);
	});
});
