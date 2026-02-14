/**
 * Web Worker for off-main-thread tile processing.
 * 
 * Handles stain normalization and enhancement without blocking the UI.
 * Uses transferable ImageData for zero-copy communication.
 */

import type { StainNormalizationMode, NormalizationParams } from './stainNormalization';
import type { StainEnhancementMode } from '$lib/stores/settings';

// ============================================================================
// Message Types
// ============================================================================

export interface ProcessTileRequest {
  type: 'process';
  id: string;
  imageData: ImageData;
  normMode: StainNormalizationMode;
  enhanceMode: StainEnhancementMode;
  normParams: NormalizationParams | null;
}

export interface ProcessTileResponse {
  type: 'processed';
  id: string;
  imageData: ImageData;
}

export interface CancelRequest {
  type: 'cancel';
  ids: string[];
}

/**
 * Message to update sharpening settings in the worker.
 * Sent from the main thread when user changes settings.
 */
export interface UpdateSettingsRequest {
  type: 'updateSettings';
  payload: {
    sharpeningEnabled: boolean;
    sharpeningIntensity: number; // 0 to 100
  };
}

export type WorkerRequest = ProcessTileRequest | CancelRequest | UpdateSettingsRequest;
export type WorkerResponse = ProcessTileResponse;

// ============================================================================
// Reference Parameters (duplicated for worker isolation)
// ============================================================================

const REFERENCE_STAIN_MATRIX_MACENKO: number[][] = [
  [0.650, 0.072],
  [0.704, 0.990],
  [0.286, 0.105],
];

const REFERENCE_MAX_C_MACENKO: number[] = [1.9705, 1.0308];

const REFERENCE_STAIN_MATRIX_VAHADANE: number[][] = [
  [0.650, 0.072],
  [0.704, 0.990],
  [0.286, 0.105],
];

const REFERENCE_MAX_C_VAHADANE: number[] = [1.9705, 1.0308];

const BACKGROUND_THRESHOLD = 240;

// ============================================================================
// Color Space Utilities
// ============================================================================

/** Small epsilon to avoid log(0) */
const OD_EPSILON = 1e-6;

/** Maximum OD value to prevent extreme values from dark pixels */
const OD_MAX = 2.5;

/**
 * Convert RGB (0-255) to Optical Density using log10.
 * Must match the implementation in stainNormalization.ts exactly.
 */
function rgbToOd(r: number, g: number, b: number): number[] {
  // Normalize to [0, 1] with epsilon to avoid log(0)
  const rNorm = Math.max(OD_EPSILON, r / 255);
  const gNorm = Math.max(OD_EPSILON, g / 255);
  const bNorm = Math.max(OD_EPSILON, b / 255);

  // Convert to optical density using log10 (via natural log / LN10)
  const odR = Math.min(OD_MAX, -Math.log(rNorm) / Math.LN10);
  const odG = Math.min(OD_MAX, -Math.log(gNorm) / Math.LN10);
  const odB = Math.min(OD_MAX, -Math.log(bNorm) / Math.LN10);

  return [odR, odG, odB];
}

/**
 * Convert Optical Density back to RGB (0-255) using 10^(-OD).
 * Must match the implementation in stainNormalization.ts exactly.
 */
function odToRgb(odR: number, odG: number, odB: number): number[] {
  // Clamp OD to reasonable range
  odR = Math.max(0, Math.min(OD_MAX, odR));
  odG = Math.max(0, Math.min(OD_MAX, odG));
  odB = Math.max(0, Math.min(OD_MAX, odB));

  // Convert back to intensity using 10^(-OD)
  const ir = Math.pow(10, -odR);
  const ig = Math.pow(10, -odG);
  const ib = Math.pow(10, -odB);

  // Scale to 0-255 and clamp
  const r = Math.max(0, Math.min(255, Math.round(ir * 255)));
  const g = Math.max(0, Math.min(255, Math.round(ig * 255)));
  const b = Math.max(0, Math.min(255, Math.round(ib * 255)));

  return [r, g, b];
}

function pseudoInverse3x2(matrix: number[][]): number[][] {
  const a = matrix[0][0], b = matrix[0][1];
  const c = matrix[1][0], d = matrix[1][1];
  const e = matrix[2][0], f = matrix[2][1];

  const ata00 = a * a + c * c + e * e;
  const ata01 = a * b + c * d + e * f;
  const ata11 = b * b + d * d + f * f;

  const det = ata00 * ata11 - ata01 * ata01;
  const invDet = Math.abs(det) > 1e-10 ? 1 / det : 0;

  const inv00 = ata11 * invDet;
  const inv01 = -ata01 * invDet;
  const inv11 = ata00 * invDet;

  return [
    [inv00 * a + inv01 * b, inv00 * c + inv01 * d, inv00 * e + inv01 * f],
    [inv01 * a + inv11 * b, inv01 * c + inv11 * d, inv01 * e + inv11 * f],
  ];
}

// ============================================================================
// HSL/Lab Utilities for Enhancement
// ============================================================================

function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  switch (max) {
    case r: h = ((g - b) / d + (g < b ? 6 : 0)) * 60; break;
    case g: h = ((b - r) / d + 2) * 60; break;
    default: h = ((r - g) / d + 4) * 60; break;
  }
  return { h, s, l };
}

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

function rgbToLab(r: number, g: number, b: number): { L: number; a: number; b: number } {
  let rLin = r / 255;
  let gLin = g / 255;
  let bLin = b / 255;
  rLin = rLin > 0.04045 ? Math.pow((rLin + 0.055) / 1.055, 2.4) : rLin / 12.92;
  gLin = gLin > 0.04045 ? Math.pow((gLin + 0.055) / 1.055, 2.4) : gLin / 12.92;
  bLin = bLin > 0.04045 ? Math.pow((bLin + 0.055) / 1.055, 2.4) : bLin / 12.92;
  let x = rLin * 0.4124564 + gLin * 0.3575761 + bLin * 0.1804375;
  let y = rLin * 0.2126729 + gLin * 0.7151522 + bLin * 0.0721750;
  let z = rLin * 0.0193339 + gLin * 0.1191920 + bLin * 0.9503041;
  x /= 0.95047;
  y /= 1.0;
  z /= 1.08883;
  const f = (t: number) => t > 0.008856 ? Math.pow(t, 1 / 3) : 7.787 * t + 16 / 116;
  const fx = f(x), fy = f(y), fz = f(z);
  return { L: 116 * fy - 16, a: 500 * (fx - fy), b: 200 * (fy - fz) };
}

function labToRgb(L: number, a: number, labB: number): { r: number; g: number; b: number } {
  const fy = (L + 16) / 116;
  const fx = a / 500 + fy;
  const fz = fy - labB / 200;
  const fInv = (t: number) => {
    const t3 = t * t * t;
    return t3 > 0.008856 ? t3 : (t - 16 / 116) / 7.787;
  };
  let x = fInv(fx) * 0.95047;
  let y = fInv(fy) * 1.0;
  let z = fInv(fz) * 1.08883;
  let rLin = x * 3.2404542 - y * 1.5371385 - z * 0.4985314;
  let gLin = -x * 0.9692660 + y * 1.8760108 + z * 0.0415560;
  let bLin = x * 0.0556434 - y * 0.2040259 + z * 1.0572252;
  const gamma = (c: number) => c > 0.0031308 ? 1.055 * Math.pow(c, 1 / 2.4) - 0.055 : 12.92 * c;
  return {
    r: Math.round(Math.max(0, Math.min(255, gamma(rLin) * 255))),
    g: Math.round(Math.max(0, Math.min(255, gamma(gLin) * 255))),
    b: Math.round(Math.max(0, Math.min(255, gamma(bLin) * 255))),
  };
}

function clamp01(v: number): number {
  return Math.max(0, Math.min(1, v));
}

// ============================================================================
// Stain Normalization
// ============================================================================

/** Maximum allowed scaling factor to prevent washed-out or over-saturated results */
const MAX_SCALE_FACTOR = 3.0;
const MIN_SCALE_FACTOR = 0.33;

function applyNormalization(
  pixels: Uint8ClampedArray,
  mode: StainNormalizationMode,
  params: NormalizationParams | null
): void {
  if (mode === 'none' || !params) return;

  const refMatrix = mode === 'macenko' ? REFERENCE_STAIN_MATRIX_MACENKO : REFERENCE_STAIN_MATRIX_VAHADANE;
  const refMaxC = mode === 'macenko' ? REFERENCE_MAX_C_MACENKO : REFERENCE_MAX_C_VAHADANE;
  
  // Compute pseudo-inverse of stain matrix
  const pinv = pseudoInverse3x2(params.stainMatrix);
  
  // Compute scaling factors with clamping to prevent extreme values
  let scale0 = params.maxC[0] > 1e-6 ? refMaxC[0] / params.maxC[0] : 1;
  let scale1 = params.maxC[1] > 1e-6 ? refMaxC[1] / params.maxC[1] : 1;
  
  // Clamp scaling factors to prevent washed-out (scale too small) or over-saturated (scale too large) results
  scale0 = Math.max(MIN_SCALE_FACTOR, Math.min(MAX_SCALE_FACTOR, scale0));
  scale1 = Math.max(MIN_SCALE_FACTOR, Math.min(MAX_SCALE_FACTOR, scale1));

  const numPixels = pixels.length / 4;
  for (let i = 0; i < numPixels; i++) {
    const offset = i * 4;
    const r = pixels[offset];
    const g = pixels[offset + 1];
    const b = pixels[offset + 2];

    if (r > BACKGROUND_THRESHOLD && g > BACKGROUND_THRESHOLD && b > BACKGROUND_THRESHOLD) {
      continue;
    }

    const od = rgbToOd(r, g, b);
    const c0 = Math.max(0, pinv[0][0] * od[0] + pinv[0][1] * od[1] + pinv[0][2] * od[2]);
    const c1 = Math.max(0, pinv[1][0] * od[0] + pinv[1][1] * od[1] + pinv[1][2] * od[2]);
    
    const c0Norm = c0 * scale0;
    const c1Norm = c1 * scale1;
    const odRefR = refMatrix[0][0] * c0Norm + refMatrix[0][1] * c1Norm;
    const odRefG = refMatrix[1][0] * c0Norm + refMatrix[1][1] * c1Norm;
    const odRefB = refMatrix[2][0] * c0Norm + refMatrix[2][1] * c1Norm;
    const rgb = odToRgb(odRefR, odRefG, odRefB);

    pixels[offset] = rgb[0];
    pixels[offset + 1] = rgb[1];
    pixels[offset + 2] = rgb[2];
  }
}

// ============================================================================
// Stain Enhancement
// ============================================================================

function applyGramEnhancement(r: number, g: number, b: number): { r: number; g: number; b: number } {
  const hsl = rgbToHsl(r, g, b);
  let { h, s, l } = hsl;
  const isPurple = h >= 240 && h <= 320 && s > 0.15;
  const isPinkRed = ((h >= 320 && h <= 360) || (h >= 0 && h <= 30)) && s > 0.2;
  const isBackground = (h >= 40 && h <= 180 && s < 0.4) || s < 0.1;

  if (isPurple) {
    s = clamp01(s * 1.35);
    l = clamp01(l * 0.9);
  } else if (isPinkRed) {
    s = clamp01(s * 1.3);
    l = clamp01(l * 0.95);
  } else if (isBackground) {
    s = clamp01(s * 0.6);
    l = clamp01(l * 1.05 + 0.02);
  }
  return hslToRgb(h, s, l);
}

function applyAfbEnhancement(r: number, g: number, b: number): { r: number; g: number; b: number } {
  const hsl = rgbToHsl(r, g, b);
  let { h, s, l } = hsl;
  const isRed = (h >= 330 || h <= 30) && s > 0.25;
  const isBlue = h >= 180 && h <= 260;

  if (isRed) {
    s = clamp01(s * 1.5);
    l = clamp01(l * 0.85);
  } else if (isBlue) {
    s = clamp01(s * 0.65);
    l = clamp01(l * 1.05);
  }
  return hslToRgb(h, s, l);
}

function applyGmsEnhancement(r: number, g: number, b: number): { r: number; g: number; b: number } {
  const lab = rgbToLab(r, g, b);
  let { L, a, b: labB } = lab;

  if (L < 50) {
    const normalized = L / 50;
    const curved = Math.pow(normalized, 1.3);
    L = curved * 50;
    a = a * 1.1;
    labB = labB * 1.1;
  } else {
    L = Math.min(100, L * 1.08 + 3);
    a = a * 0.6;
    labB = labB * 0.6;
  }
  return labToRgb(L, a, labB);
}

function applyEnhancement(
  pixels: Uint8ClampedArray,
  mode: StainEnhancementMode
): void {
  if (mode === 'none') return;

  const numPixels = pixels.length / 4;
  for (let i = 0; i < numPixels; i++) {
    const offset = i * 4;
    const r = pixels[offset];
    const g = pixels[offset + 1];
    const b = pixels[offset + 2];

    let result: { r: number; g: number; b: number };
    switch (mode) {
      case 'gram':
        result = applyGramEnhancement(r, g, b);
        break;
      case 'afb':
        result = applyAfbEnhancement(r, g, b);
        break;
      case 'gms':
        result = applyGmsEnhancement(r, g, b);
        break;
      default:
        continue;
    }

    pixels[offset] = result.r;
    pixels[offset + 1] = result.g;
    pixels[offset + 2] = result.b;
  }
}

// ============================================================================
// Worker Entry Point
// ============================================================================

const cancelledIds = new Set<string>();
/** Maximum cancelled IDs to track before cleanup (prevents memory leak) */
const MAX_CANCELLED_IDS = 500;

// ============================================================================
// Sharpening Settings (Worker-Local State)
// ============================================================================

/**
 * Worker-local sharpening configuration.
 * Updated via 'updateSettings' messages from main thread.
 */
let sharpeningEnabled = false;
let sharpeningIntensity = 50; // 0 to 100

// ============================================================================
// Luminance-Only Unsharp Mask Sharpening
// ============================================================================

/**
 * Apply luminance-only unsharp mask sharpening to RGBA pixel data.
 * 
 * This algorithm sharpens by enhancing local contrast in the luminance channel
 * while preserving original color ratios. This approach is preferred for
 * histology images because:
 * 
 * 1. It avoids color fringing artifacts that occur when sharpening RGB channels
 *    independently
 * 2. It preserves stain colors accurately, which is critical for diagnosis
 * 3. It enhances structural details (cell boundaries, tissue architecture)
 *    without altering the perceived stain intensity ratios
 * 
 * Algorithm:
 * 1. Extract luminance from each pixel using standard weights
 * 2. Blur luminance with a 3x3 Gaussian-like kernel
 * 3. Compute detail = original_luminance - blurred_luminance
 * 4. Add scaled detail back: sharpened_luminance = original + amount * detail
 * 5. Scale RGB proportionally to match the new luminance
 * 
 * @param pixels - RGBA pixel data (modified in-place)
 * @param width - Image width in pixels
 * @param height - Image height in pixels
 * @param intensity - Sharpening intensity from 0 to 100
 */
function applyUnsharpMaskLuminance(
  pixels: Uint8ClampedArray,
  width: number,
  height: number,
  intensity: number
): void {
  // Map intensity [0, 100] to internal amount [0.0, 0.8]
  // Allows stronger sharpening for histology images when needed
  const amount = 0.8 * (intensity / 100);
  
  if (amount <= 0) return;

  // Luminance weights (Rec. 601 luma)
  const wR = 0.299;
  const wG = 0.587;
  const wB = 0.114;

  const numPixels = width * height;

  // Step 1: Compute original luminance for all pixels
  const luminance = new Float32Array(numPixels);
  for (let i = 0; i < numPixels; i++) {
    const offset = i * 4;
    const r = pixels[offset];
    const g = pixels[offset + 1];
    const b = pixels[offset + 2];
    luminance[i] = wR * r + wG * g + wB * b;
  }

  // Step 2: Compute blurred luminance using 3x3 Gaussian-like kernel
  // Kernel: [1,2,1; 2,4,2; 1,2,1] / 16
  const kernel = [1, 2, 1, 2, 4, 2, 1, 2, 1];
  const blurred = new Float32Array(numPixels);

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let acc = 0;
      let wSum = 0;

      // Apply 3x3 kernel
      for (let ky = -1; ky <= 1; ky++) {
        for (let kx = -1; kx <= 1; kx++) {
          const ix = x + kx;
          const iy = y + ky;

          // Skip out-of-bounds pixels (edge handling)
          if (ix < 0 || ix >= width || iy < 0 || iy >= height) continue;

          const weight = kernel[(ky + 1) * 3 + (kx + 1)];
          const srcIdx = iy * width + ix;

          acc += luminance[srcIdx] * weight;
          wSum += weight;
        }
      }

      blurred[y * width + x] = acc / (wSum || 1);
    }
  }

  // Step 3 & 4: Apply unsharp mask and scale RGB
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const idx1D = y * width + x;
      const offset = idx1D * 4;

      const origY = luminance[idx1D];
      const blurY = blurred[idx1D];
      
      // Detail is the high-frequency component
      const detail = origY - blurY;

      // Sharpened luminance with unsharp mask formula
      let ySharp = origY + amount * detail;

      // Clamp to valid luminance range
      if (ySharp < 0) ySharp = 0;
      if (ySharp > 255) ySharp = 255;

      // Step 5: Scale RGB proportionally to preserve color ratios
      const r = pixels[offset];
      const g = pixels[offset + 1];
      const b = pixels[offset + 2];

      // Avoid division by zero for very dark pixels
      const yOrigSafe = origY > 0.001 ? origY : 0.001;
      const scale = ySharp / yOrigSafe;

      // Apply scale and clamp
      let rNew = r * scale;
      let gNew = g * scale;
      let bNew = b * scale;

      if (rNew < 0) rNew = 0; if (rNew > 255) rNew = 255;
      if (gNew < 0) gNew = 0; if (gNew > 255) gNew = 255;
      if (bNew < 0) bNew = 0; if (bNew > 255) bNew = 255;

      pixels[offset] = rNew;
      pixels[offset + 1] = gNew;
      pixels[offset + 2] = bNew;
      // Alpha channel remains unchanged
    }
  }
}

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  const request = event.data;

  // Handle settings update messages
  if (request.type === 'updateSettings') {
    sharpeningEnabled = request.payload.sharpeningEnabled;
    sharpeningIntensity = Math.max(0, Math.min(100, request.payload.sharpeningIntensity));
    return;
  }

  if (request.type === 'cancel') {
    for (const id of request.ids) {
      cancelledIds.add(id);
    }
    // Prevent unbounded growth - clear oldest entries when over limit
    if (cancelledIds.size > MAX_CANCELLED_IDS) {
      const idsArray = Array.from(cancelledIds);
      const toRemove = idsArray.slice(0, cancelledIds.size - MAX_CANCELLED_IDS / 2);
      for (const id of toRemove) {
        cancelledIds.delete(id);
      }
    }
    return;
  }

  if (request.type === 'process') {
    const { id, imageData, normMode, enhanceMode, normParams } = request;

    // Check if cancelled before processing
    if (cancelledIds.has(id)) {
      cancelledIds.delete(id);
      return;
    }

    // Process pixels in-place
    applyNormalization(imageData.data, normMode, normParams);
    
    // Check cancellation between stages
    if (cancelledIds.has(id)) {
      cancelledIds.delete(id);
      return;
    }

    applyEnhancement(imageData.data, enhanceMode);

    // Check cancellation before sharpening
    if (cancelledIds.has(id)) {
      cancelledIds.delete(id);
      return;
    }

    // Apply sharpening as final step (after normalization and enhancement)
    // This preserves the stain color accuracy while enhancing structural details
    if (sharpeningEnabled && sharpeningIntensity > 0) {
      applyUnsharpMaskLuminance(
        imageData.data,
        imageData.width,
        imageData.height,
        sharpeningIntensity
      );
    }

    // Send back with transferable
    const response: ProcessTileResponse = {
      type: 'processed',
      id,
      imageData,
    };

    self.postMessage(response, { transfer: [imageData.data.buffer] });
  }
};
