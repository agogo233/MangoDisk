import { describe, expect, it } from 'vitest';
import { cpuIdentityLabel } from './cpu-identity-label';
describe('CPU identity caption', () => {
  it('preserves the manufacturer clock without duplicating it', () => {
    expect(cpuIdentityLabel({ model: 'Intel Core i9 CPU @ 3.70GHz', nominalFrequencyMhz: 3701 })).toBe(
      'Intel Core i9 CPU @ 3.70GHz'
    );
  });
  it('shows only a known nominal clock and never invents one', () => {
    expect(cpuIdentityLabel({ model: 'Apple M3 Max', nominalFrequencyMhz: null })).toBe('Apple M3 Max');
    expect(cpuIdentityLabel({ model: 'AMD Ryzen', nominalFrequencyMhz: 3800 })).toBe('AMD Ryzen · 3.80 GHz');
    expect(cpuIdentityLabel({ model: 'CPU', nominalFrequencyMhz: Number.NaN })).toBe('CPU');
    expect(cpuIdentityLabel(null)).toBe('');
  });
});
