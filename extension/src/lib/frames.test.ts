import { describe, expect, it } from 'vitest';
import { chunk, end, readData } from './frames';

describe('frames', () => {
	it('reads what it writes, as the box does', () => {
		const c = chunk(7, new Uint8Array([104, 105]));
		expect(Array.from(c)).toEqual([0, 0, 0, 7, 0, 104, 105]);
		const r = readData(c.buffer as ArrayBuffer);
		expect(r && !r.end && Array.from(r.bytes)).toEqual([104, 105]);
		expect(readData(end(7).buffer as ArrayBuffer)).toEqual({ id: 7, end: true });
		expect(readData(new Uint8Array([0, 0]).buffer)).toBeNull();
	});
});
