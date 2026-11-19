import { describe, expect, it } from 'vitest';
import { homeFor, updated, type Box } from './box';

const box: Box = {
	fqdn: 'maplehouse.zoomriya.app',
	boxKey: 'k',
	device: 'd',
	pass: 'old',
	until: 1,
	server: 'https://connect.zoomriya.net',
	home: 'https://maplehouse.zoomriya.app:7445',
	name: 'Laptop',
	relays: ['https://relay1.zoomriya.net']
};

describe('box', () => {
	it('takes a fresh pass and the relays now', () => {
		const b = updated(box, { pass: 'new', until: 9, relays: ['https://relay2.zoomriya.net'], ice: [] });
		expect(b.pass).toBe('new');
		expect(b.until).toBe(9);
		expect(b.relays).toEqual(['https://relay2.zoomriya.net']);
		expect(b.home).toBe(box.home);
	});

	it('keeps what an answer left out', () => {
		const b = updated(box, { relays: [] });
		expect(b.pass).toBe('old');
		expect(b.relays).toEqual(box.relays);
	});

	it('finds the home address', () => {
		expect(homeFor('home.smith.ca')).toBe('https://home.smith.ca:7445');
		expect(homeFor('http://127.0.0.1:7445/')).toBe('http://127.0.0.1:7445');
		expect(homeFor('not a name')).toBeNull();
	});
});
