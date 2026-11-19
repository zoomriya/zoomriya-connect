import { describe, expect, it } from 'vitest';
import { b64url, fromB64url, fromZ32, z32 } from './codec';

describe('codec', () => {
	it('writes z-base-32 like iroh', () => {
		// The all-zero key, and one byte each way.
		expect(z32(new Uint8Array(32))).toBe('y'.repeat(52));
		const k = new Uint8Array(32).map((_, i) => i * 7 + 3);
		expect(fromZ32(z32(k), 32)).toEqual(k);
		expect(fromZ32('not z32!', 32)).toBeNull();
	});
	it('round-trips base64url', () => {
		const b = new Uint8Array([0, 251, 255, 1, 62, 63]);
		expect(b64url(b)).not.toMatch(/[+/=]/);
		expect(fromB64url(b64url(b))).toEqual(b);
	});
});
