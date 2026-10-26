import { describe, expect, it } from 'vitest';
import { sameMembers } from './set-membership';

describe('set membership', () => {
  it('ignores ordering and repeated values without changing either input', () => {
    const left = ['b', 'a', 'a'];
    const right = ['a', 'b'];
    expect(sameMembers(left, right)).toBe(true);
    expect(left).toEqual(['b', 'a', 'a']);
    expect(right).toEqual(['a', 'b']);
  });

  it('requires identical members, including for empty selections', () => {
    expect(sameMembers([], [])).toBe(true);
    expect(sameMembers([], ['a'])).toBe(false);
    expect(sameMembers(['a'], [])).toBe(false);
    expect(sameMembers(['a', 'b'], ['a', 'c'])).toBe(false);
    expect(sameMembers(new Set(['a', 'b']), ['b', 'a'])).toBe(true);
  });
});
