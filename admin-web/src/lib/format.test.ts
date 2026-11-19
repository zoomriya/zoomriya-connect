import { describe, expect, it } from 'vitest';
import { ago, count, shortKey, size } from './format';

describe('format', () => {
	const now = 1_800_000_000;
	it('says how long ago', () => {
		expect(ago(now - 5, now, 'en')).toBe('now');
		expect(ago(now - 180, now, 'en')).toBe('3 minutes ago');
		expect(ago(now - 86400, now, 'en')).toBe('yesterday');
		expect(ago(now - 180, now, 'fr')).toBe('il y a 3 minutes');
	});
	it('groups numbers', () => {
		expect(count(12345, 'en')).toBe('12,345');
		expect(count(12345, 'fr').replace(/\s/g, ' ')).toBe('12 345');
	});
	it('shortens keys', () => {
		expect(shortKey('yxyzk3jcu7awqmfs6igpxg5ky9d57zrp')).toBe('yxyzk3…7zrp');
		expect(shortKey('short')).toBe('short');
	});
	it('says sizes', () => {
		expect(size(950_000_000, 'en')).toBe('950\u00a0MB');
		expect(size(3_240_000_000, 'en')).toBe('3.2\u00a0GB');
		expect(size(3_240_000_000, 'fr')).toBe('3,2\u00a0Go');
		expect(size(0, 'en')).toBe('0\u00a0B');
	});
});
