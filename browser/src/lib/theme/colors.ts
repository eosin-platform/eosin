/**
 * Eosin Design System Color Palette
 * 
 * Use these constants for programmatic color access in TypeScript.
 * CSS should use var(--primary-hex), var(--secondary-hex), etc.
 */

export const colors = {
  primary: {
    DEFAULT: '#5E4AEF',
    rgb: { r: 94, g: 74, b: 239 },
    foreground: '#FFFFFF',
    muted: 'rgba(94, 74, 239, 0.2)',
    hover: '#4C3BD9',
  },
  secondary: {
    DEFAULT: '#FE0E94',
    rgb: { r: 254, g: 14, b: 148 },
    foreground: '#FFFFFF',
    muted: 'rgba(254, 14, 148, 0.2)',
    hover: '#E60A82',
  },
  measurement: {
    DEFAULT: '#17CC00',
    rgb: { r: 23, g: 204, b: 0 },
    foreground: '#FFFFFF',
  },
} as const;

// Convenience exports
export const PRIMARY = colors.primary.DEFAULT;
export const PRIMARY_RGB = colors.primary.rgb;
export const SECONDARY = colors.secondary.DEFAULT;
export const SECONDARY_RGB = colors.secondary.rgb;
export const MEASUREMENT = colors.measurement.DEFAULT;
export const MEASUREMENT_RGB = colors.measurement.rgb;
