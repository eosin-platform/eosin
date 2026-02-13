/**
 * Stain Enhancement Module
 * 
 * Provides post-processing color enhancements for histology slides
 * to improve visibility of specific stain types:
 * - Gram: Enhances purple/pink bacteria visibility
 * - AFB: Highlights red acid-fast bacilli against blue background
 * - GMS: Enhances dark fungal elements against pale background
 * 
 * Uses color-space heuristics (HSL/Lab) for fast, pragmatic enhancements.
 */

import type { StainEnhancementMode } from '$lib/stores/settings';

// ============================================================================
// Color Space Conversion Utilities
// ============================================================================

/**
 * Convert RGB (0-255) to HSL (h: 0-360, s: 0-1, l: 0-1)
 */
function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  r /= 255;
  g /= 255;
  b /= 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;

  if (max === min) {
    return { h: 0, s: 0, l };
  }

  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);

  let h: number;
  switch (max) {
    case r:
      h = ((g - b) / d + (g < b ? 6 : 0)) * 60;
      break;
    case g:
      h = ((b - r) / d + 2) * 60;
      break;
    default:
      h = ((r - g) / d + 4) * 60;
      break;
  }

  return { h, s, l };
}

/**
 * Convert HSL (h: 0-360, s: 0-1, l: 0-1) to RGB (0-255)
 */
function hslToRgb(h: number, s: number, l: number): { r: number; g: number; b: number } {
  if (s === 0) {
    const gray = Math.round(l * 255);
    return { r: gray, g: gray, b: gray };
  }

  const hueToRgb = (p: number, q: number, t: number): number => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };

  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const hNorm = h / 360;

  return {
    r: Math.round(hueToRgb(p, q, hNorm + 1 / 3) * 255),
    g: Math.round(hueToRgb(p, q, hNorm) * 255),
    b: Math.round(hueToRgb(p, q, hNorm - 1 / 3) * 255),
  };
}

/**
 * Convert sRGB (0-255) to approximate Lab color space.
 * Uses D65 illuminant. Simplified for performance.
 */
function rgbToLab(r: number, g: number, b: number): { L: number; a: number; b: number } {
  // sRGB to linear RGB
  let rLin = r / 255;
  let gLin = g / 255;
  let bLin = b / 255;

  // Apply gamma correction
  rLin = rLin > 0.04045 ? Math.pow((rLin + 0.055) / 1.055, 2.4) : rLin / 12.92;
  gLin = gLin > 0.04045 ? Math.pow((gLin + 0.055) / 1.055, 2.4) : gLin / 12.92;
  bLin = bLin > 0.04045 ? Math.pow((bLin + 0.055) / 1.055, 2.4) : bLin / 12.92;

  // RGB to XYZ (D65)
  let x = rLin * 0.4124564 + gLin * 0.3575761 + bLin * 0.1804375;
  let y = rLin * 0.2126729 + gLin * 0.7151522 + bLin * 0.0721750;
  let z = rLin * 0.0193339 + gLin * 0.1191920 + bLin * 0.9503041;

  // Normalize for D65 white point
  x /= 0.95047;
  y /= 1.0;
  z /= 1.08883;

  // XYZ to Lab
  const epsilon = 0.008856;
  const kappa = 903.3;

  const fx = x > epsilon ? Math.pow(x, 1 / 3) : (kappa * x + 16) / 116;
  const fy = y > epsilon ? Math.pow(y, 1 / 3) : (kappa * y + 16) / 116;
  const fz = z > epsilon ? Math.pow(z, 1 / 3) : (kappa * z + 16) / 116;

  return {
    L: 116 * fy - 16,
    a: 500 * (fx - fy),
    b: 200 * (fy - fz),
  };
}

/**
 * Convert Lab to sRGB (0-255)
 */
function labToRgb(L: number, a: number, labB: number): { r: number; g: number; b: number } {
  // Lab to XYZ
  const fy = (L + 16) / 116;
  const fx = a / 500 + fy;
  const fz = fy - labB / 200;

  const epsilon = 0.008856;
  const kappa = 903.3;

  const xr = Math.pow(fx, 3) > epsilon ? Math.pow(fx, 3) : (116 * fx - 16) / kappa;
  const yr = L > kappa * epsilon ? Math.pow((L + 16) / 116, 3) : L / kappa;
  const zr = Math.pow(fz, 3) > epsilon ? Math.pow(fz, 3) : (116 * fz - 16) / kappa;

  // D65 white point
  const x = xr * 0.95047;
  const y = yr * 1.0;
  const z = zr * 1.08883;

  // XYZ to linear RGB
  let rLin = x * 3.2404542 + y * -1.5371385 + z * -0.4985314;
  let gLin = x * -0.9692660 + y * 1.8760108 + z * 0.0415560;
  let bLin = x * 0.0556434 + y * -0.2040259 + z * 1.0572252;

  // Linear RGB to sRGB
  const toSrgb = (c: number): number => {
    c = Math.max(0, Math.min(1, c));
    return c > 0.0031308 ? 1.055 * Math.pow(c, 1 / 2.4) - 0.055 : 12.92 * c;
  };

  return {
    r: Math.round(toSrgb(rLin) * 255),
    g: Math.round(toSrgb(gLin) * 255),
    b: Math.round(toSrgb(bLin) * 255),
  };
}

/**
 * Clamp a value to [0, 255]
 */
function clamp255(v: number): number {
  return Math.max(0, Math.min(255, Math.round(v)));
}

/**
 * Clamp a value to [0, 1]
 */
function clamp01(v: number): number {
  return Math.max(0, Math.min(1, v));
}

// ============================================================================
// Stain Enhancement Algorithms
// ============================================================================

/**
 * Gram Stain Enhancement
 * 
 * Enhances Gram-positive (purple) and Gram-negative (pink/red) bacteria
 * while toning down background tissue.
 * 
 * - Purple range: hue ~260°–320°, boost saturation, darken slightly
 * - Pink/red range: hue ~330°–30°, boost saturation
 * - Background (yellow/green, low sat): reduce saturation, lighten
 */
function applyGramEnhancement(r: number, g: number, b: number): { r: number; g: number; b: number } {
  const hsl = rgbToHsl(r, g, b);
  let { h, s, l } = hsl;

  // Identify purple/magenta (Gram-positive): hue 260-320
  const isPurple = h >= 260 && h <= 320 && s > 0.15;
  
  // Identify pink/red (Gram-negative): hue 330-360 or 0-30
  const isPinkRed = (h >= 330 || h <= 30) && s > 0.2;

  // Identify background-like colors (yellow/green, low saturation)
  const isBackground = (h >= 40 && h <= 180 && s < 0.4) || s < 0.1;

  if (isPurple) {
    // Boost saturation and darken slightly for better visibility
    s = clamp01(s * 1.35);
    l = clamp01(l * 0.9); // Darken slightly
  } else if (isPinkRed) {
    // Boost saturation for pink bacteria
    s = clamp01(s * 1.3);
    l = clamp01(l * 0.95);
  } else if (isBackground) {
    // Reduce saturation and lighten background
    s = clamp01(s * 0.6);
    l = clamp01(l * 1.05 + 0.02);
  }

  return hslToRgb(h, s, l);
}

/**
 * AFB (Ziehl-Neelsen) Stain Enhancement
 * 
 * Highlights red acid-fast bacilli against blue methylene blue background.
 * 
 * - Red/magenta pixels (hue ~330°–30°, high sat): boost saturation, darken
 * - Blue background: reduce saturation for uniform appearance
 */
function applyAfbEnhancement(r: number, g: number, b: number): { r: number; g: number; b: number } {
  const hsl = rgbToHsl(r, g, b);
  let { h, s, l } = hsl;

  // Identify red/magenta (acid-fast bacilli): hue 330-360 or 0-30
  const isRed = (h >= 330 || h <= 30) && s > 0.25;

  // Identify blue background: hue 180-260
  const isBlue = h >= 180 && h <= 260;

  if (isRed) {
    // Strongly boost saturation and contrast for bacilli
    s = clamp01(s * 1.5);
    l = clamp01(l * 0.85); // Darken to make them pop
  } else if (isBlue) {
    // Reduce saturation of blue background for cleaner appearance
    s = clamp01(s * 0.65);
    l = clamp01(l * 1.05);
  }

  return hslToRgb(h, s, l);
}

/**
 * GMS (Grocott's Methenamine Silver) Stain Enhancement
 * 
 * Enhances dark fungal elements (black/brown from silver staining)
 * against pale green/yellow background.
 * 
 * Uses Lab color space for better lightness-based discrimination:
 * - Low L (dark structures): increase contrast, push darker
 * - High L (pale background): lighten and desaturate
 */
function applyGmsEnhancement(r: number, g: number, b: number): { r: number; g: number; b: number } {
  const lab = rgbToLab(r, g, b);
  let { L, a, b: labB } = lab;

  // Dark structures (fungi, silver deposits): L < 50
  // Apply a contrast curve to make them more distinct
  if (L < 50) {
    // Push dark values darker using a power curve
    // This creates a contrast boost in the dark region
    const normalized = L / 50; // 0 to 1
    const curved = Math.pow(normalized, 1.3); // Power curve for contrast
    L = curved * 50;
    
    // Slightly boost chromatic channels for brown/black definition
    a = a * 1.1;
    labB = labB * 1.1;
  } else {
    // Pale background: lighten and reduce saturation
    L = Math.min(100, L * 1.08 + 3);
    
    // Reduce chromatic components (desaturate)
    a = a * 0.6;
    labB = labB * 0.6;
  }

  return labToRgb(L, a, labB);
}

// ============================================================================
// Main API
// ============================================================================

/**
 * Apply stain enhancement to a single RGB pixel.
 * 
 * @param rgb - Input color with r, g, b values (0-255)
 * @param mode - Enhancement mode: 'none', 'gram', 'afb', or 'gms'
 * @returns Enhanced RGB color
 */
export function applyStainEnhancement(
  rgb: { r: number; g: number; b: number },
  mode: StainEnhancementMode
): { r: number; g: number; b: number } {
  switch (mode) {
    case 'none':
      return rgb;
    case 'gram':
      return applyGramEnhancement(rgb.r, rgb.g, rgb.b);
    case 'afb':
      return applyAfbEnhancement(rgb.r, rgb.g, rgb.b);
    case 'gms':
      return applyGmsEnhancement(rgb.r, rgb.g, rgb.b);
    default:
      return rgb;
  }
}

/**
 * Apply stain enhancement to an ImageData buffer in-place.
 * This is more efficient than per-pixel calls for canvas-based rendering.
 * 
 * @param imageData - Canvas ImageData to modify in-place
 * @param mode - Enhancement mode
 */
export function applyStainEnhancementToImageData(
  imageData: ImageData,
  mode: StainEnhancementMode
): void {
  if (mode === 'none') return;

  const data = imageData.data;
  const len = data.length;

  for (let i = 0; i < len; i += 4) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    // alpha at data[i + 3] is preserved

    const enhanced = applyStainEnhancement({ r, g, b }, mode);
    data[i] = clamp255(enhanced.r);
    data[i + 1] = clamp255(enhanced.g);
    data[i + 2] = clamp255(enhanced.b);
  }
}

/**
 * Create an enhanced ImageBitmap from a source bitmap.
 * Uses OffscreenCanvas for processing if available.
 * 
 * @param source - Source ImageBitmap
 * @param mode - Enhancement mode
 * @returns Promise resolving to enhanced ImageBitmap
 */
export async function createEnhancedBitmap(
  source: ImageBitmap,
  mode: StainEnhancementMode
): Promise<ImageBitmap> {
  if (mode === 'none') {
    return source;
  }

  const width = source.width;
  const height = source.height;

  // Use OffscreenCanvas if available (better performance)
  if (typeof OffscreenCanvas !== 'undefined') {
    const offscreen = new OffscreenCanvas(width, height);
    const ctx = offscreen.getContext('2d');
    if (!ctx) throw new Error('Failed to get 2D context');

    ctx.drawImage(source, 0, 0);
    const imageData = ctx.getImageData(0, 0, width, height);
    applyStainEnhancementToImageData(imageData, mode);
    ctx.putImageData(imageData, 0, 0);

    return createImageBitmap(offscreen);
  }

  // Fallback to regular canvas
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('Failed to get 2D context');

  ctx.drawImage(source, 0, 0);
  const imageData = ctx.getImageData(0, 0, width, height);
  applyStainEnhancementToImageData(imageData, mode);
  ctx.putImageData(imageData, 0, 0);

  return createImageBitmap(canvas);
}
