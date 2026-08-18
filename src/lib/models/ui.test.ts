import { describe, expect, it } from 'vitest';

import { ICON_NAMES } from './ui';

describe('UI icon names', () => {
  it('provides the copy icon used by inline actions', () => {
    expect(ICON_NAMES.copy).toBe('copy');
  });
});
