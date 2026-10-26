import type { ResidentPreferences } from '@/lib/models/resident/resident';

export const FLOATING_FONT_SCALES = { standard: 1, larger: 1.15, large: 1.3, extraLarge: 1.5 } as const;
export const FLOATING_FONT_SIZES = ['standard', 'larger', 'large', 'extraLarge'] as const;
export const FLOATING_BACKGROUNDS = [
  'default',
  'darkGray',
  'inkBlue',
  'warmWhite',
  'lightGray',
  'black',
  'white',
] as const;
export type FloatingTypography = Pick<ResidentPreferences['floating'], 'fontSize' | 'bold'>;
