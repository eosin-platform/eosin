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

export type WorkerRequest = ProcessTileRequest | CancelRequest;
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

function rgbToOd(r: number, g: number, b: number): number[] {
  const rNorm = Math.max(r, 1) / 255;
  const gNorm = Math.max(g, 1) / 255;
  const bNorm = Math.max(b, 1) / 255;
  return [
    -Math.log(rNorm),
    -Math.log(gNorm),
    -Math.log(bNorm),
  ];
}

function odToRgb(odR: number, odG: number, odB: number): number[] {
  const r = Math.round(Math.max(0, Math.min(255, 255 * Math.exp(-odR))));
  const g = Math.round(Math.max(0, Math.min(255, 255 * Math.exp(-odG))));
  const b = Math.round(Math.max(0, Math.min(255, 255 * Math.exp(-odB))));
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

function applyNormalization(
  pixels: Uint8ClampedArray,
  mode: StainNormalizationMode,
  params: NormalizationParams | null
): void {
  if (mode === 'none' || !params) return;

  const refMatrix = mode === 'macenko' ? REFERENCE_STAIN_MATRIX_MACENKO : REFERENCE_STAIN_MATRIX_VAHADANE;
  const refMaxC = mode === 'macenko' ? REFERENCE_MAX_C_MACENKO : REFERENCE_MAX_C_VAHADANE;
  const pinv = pseudoInverse3x2(params.stainMatrix);
  const scale0 = params.maxC[0] > 1e-6 ? refMaxC[0] / params.maxC[0] : 1;
  const scale1 = params.maxC[1] > 1e-6 ? refMaxC[1] / params.maxC[1] : 1;

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
    const c0 = pinv[0][0] * od[0] + pinv[0][1] * od[1] + pinv[0][2] * od[2];
    const c1 = pinv[1][0] * od[0] + pinv[1][1] * od[1] + pinv[1][2] * od[2];
    const c0Norm = Math.max(0, c0) * scale0;
    const c1Norm = Math.max(0, c1) * scale1;
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

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  const request = event.data;

  if (request.type === 'cancel') {
    for (const id of request.ids) {
      cancelledIds.add(id);
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

    // Send back with transferable
    const response: ProcessTileResponse = {
      type: 'processed',
      id,
      imageData,
    };

    self.postMessage(response, { transfer: [imageData.data.buffer] });
  }
};
