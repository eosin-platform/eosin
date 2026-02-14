/**
 * Stain Normalization Module
 *
 * Implements Macenko and Vahadane stain normalization for H&E histology slides.
 * These algorithms standardize color appearance across slides by:
 * 1. Estimating slide-specific stain vectors in optical density (OD) space
 * 2. Mapping stain concentrations to a reference distribution
 *
 * This enables consistent visualization regardless of staining variability.
 *
 * References:
 * - Macenko et al., "A method for normalizing histology slides for quantitative analysis"
 * - Vahadane et al., "Structure-preserving color normalization and sparse stain separation"
 */

import type { StainNormalization } from '$lib/stores/settings';

// Re-export for convenience
export type StainNormalizationMode = StainNormalization;

// ============================================================================
// Types
// ============================================================================

export interface RGB {
  r: number;
  g: number;
  b: number;
}

export interface NormalizationParams {
  /** 3x2 stain matrix: columns are H and E stain vectors in OD space */
  stainMatrix: number[][];
  /** Maximum stain concentrations (99th percentile) for each stain */
  maxC: number[];
  /** Mode used to compute these params */
  mode: StainNormalizationMode;
}

// ============================================================================
// Reference Parameters for H&E
// ============================================================================

/**
 * Reference stain matrix for Macenko normalization.
 * Columns are [Hematoxylin, Eosin] stain vectors in OD space.
 * Each column is a unit-normalized stain vector in OD (optical density) space.
 * 
 * In OD space, higher values mean MORE light absorption at that wavelength.
 * The transmitted color is the inverse: high OD_R + OD_G but low OD_B → appears blue.
 * 
 * - Hematoxylin: [0.650, 0.704, 0.286] - Blue-purple stain
 * - Eosin: [0.072, 0.990, 0.105] - Pink stain
 */
const REFERENCE_STAIN_MATRIX_MACENKO: number[][] = [
  // [Hematoxylin, Eosin] - OD values per RGB channel
  [0.650, 0.072],  // OD_R
  [0.704, 0.990],  // OD_G
  [0.286, 0.105],  // OD_B
];

/**
 * Reference maximum stain concentrations for Macenko.
 * These represent the 99th percentile of stain intensities in a well-stained reference slide.
 * Used to scale concentrations to match the reference appearance.
 */
const REFERENCE_MAX_C_MACENKO: number[] = [1.9705, 1.0308];

/**
 * Reference stain matrix for Vahadane normalization.
 * Uses the same reference as Macenko for consistency.
 */
const REFERENCE_STAIN_MATRIX_VAHADANE: number[][] = [
  [0.650, 0.072],
  [0.704, 0.990],
  [0.286, 0.105],
];

/** Reference maximum stain concentrations for Vahadane */
const REFERENCE_MAX_C_VAHADANE: number[] = [1.9705, 1.0308];

// ============================================================================
// Normalization Parameter Cache & Multi-Tile Sampling
// ============================================================================

/**
 * Cache for computed normalization parameters.
 * Key format: `${slideId}_${mode}` to support different modes per slide.
 */
const normalizationCache = new Map<string, NormalizationParams>();

/**
 * Accumulated OD samples from multiple tiles, keyed by slideId_mode.
 * Used to build up enough samples before computing final parameters.
 */
const sampleAccumulator = new Map<string, number[][]>();

/** Minimum number of OD samples needed for reliable parameter estimation */
const MIN_SAMPLES_FOR_ESTIMATION = 5000;

/** Maximum samples to accumulate (reduced from 100k for memory efficiency) */
const MAX_ACCUMULATED_SAMPLES = 15000;

/** Number of tiles we've sampled from, for tracking */
const tilesContributed = new Map<string, number>();

/** Minimum number of tiles required before computing parameters */
const MIN_TILES_FOR_ESTIMATION = 8;

/**
 * Interval of tiles at which to compute candidate parameters for stability checking.
 * E.g., every 4 tiles we recompute and compare to cached params.
 */
const STABILITY_CHECK_INTERVAL = 4;

/**
 * Maximum relative difference in stain matrix or maxC before invalidating cache.
 * If new candidate params differ by more than this threshold, we invalidate and re-estimate.
 */
const STABILITY_THRESHOLD = 0.25;

/** Track consecutive stable checks to know when we can clear the accumulator */
const stableCheckCount = new Map<string, number>();

/** Number of consecutive stable checks before clearing accumulator */
const STABLE_CHECKS_BEFORE_CLEANUP = 3;

/** Track last stability check tile count per cache key */
const lastStabilityCheck = new Map<string, number>();

/**
 * Clear cached normalization parameters for a specific slide or all slides.
 * @param slideId - Optional slide ID. If omitted, clears all cached params.
 */
export function clearNormalizationCache(slideId?: string): void {
  if (slideId) {
    for (const key of normalizationCache.keys()) {
      if (key.startsWith(`${slideId}_`)) {
        normalizationCache.delete(key);
      }
    }
    for (const key of sampleAccumulator.keys()) {
      if (key.startsWith(`${slideId}_`)) {
        sampleAccumulator.delete(key);
      }
    }
    for (const key of tilesContributed.keys()) {
      if (key.startsWith(`${slideId}_`)) {
        tilesContributed.delete(key);
      }
    }
    for (const key of lastStabilityCheck.keys()) {
      if (key.startsWith(`${slideId}_`)) {
        lastStabilityCheck.delete(key);
      }
    }
    for (const key of stableCheckCount.keys()) {
      if (key.startsWith(`${slideId}_`)) {
        stableCheckCount.delete(key);
      }
    }
  } else {
    normalizationCache.clear();
    sampleAccumulator.clear();
    tilesContributed.clear();
    lastStabilityCheck.clear();
    stableCheckCount.clear();
  }
}

// ============================================================================
// Color Space Conversion: RGB ↔ Optical Density (OD)
// ============================================================================

/** Small epsilon to avoid log(0) */
const OD_EPSILON = 1e-6;

/** Maximum OD value to prevent extreme values from dark pixels */
const OD_MAX = 2.5;

/**
 * Convert RGB (0-255) to Optical Density.
 * OD = -log10(I), where I = rgb/255 is normalized intensity.
 */
function rgbToOd(r: number, g: number, b: number): [number, number, number] {
  // Normalize to [0, 1] with epsilon to avoid log(0)
  const ir = Math.max(OD_EPSILON, r / 255);
  const ig = Math.max(OD_EPSILON, g / 255);
  const ib = Math.max(OD_EPSILON, b / 255);

  // Convert to optical density (using natural log, then scale to log10)
  const odR = Math.min(OD_MAX, -Math.log(ir) / Math.LN10);
  const odG = Math.min(OD_MAX, -Math.log(ig) / Math.LN10);
  const odB = Math.min(OD_MAX, -Math.log(ib) / Math.LN10);

  return [odR, odG, odB];
}

/**
 * Convert Optical Density back to RGB (0-255).
 * I = 10^(-OD), then rgb = I * 255
 */
function odToRgb(odR: number, odG: number, odB: number): [number, number, number] {
  // Clamp OD to reasonable range
  odR = Math.max(0, Math.min(OD_MAX, odR));
  odG = Math.max(0, Math.min(OD_MAX, odG));
  odB = Math.max(0, Math.min(OD_MAX, odB));

  // Convert back to intensity
  const ir = Math.pow(10, -odR);
  const ig = Math.pow(10, -odG);
  const ib = Math.pow(10, -odB);

  // Scale to 0-255 and clamp
  return [
    Math.max(0, Math.min(255, Math.round(ir * 255))),
    Math.max(0, Math.min(255, Math.round(ig * 255))),
    Math.max(0, Math.min(255, Math.round(ib * 255))),
  ];
}

// ============================================================================
// OD Sampling from Pixel Buffer
// ============================================================================

/** Threshold to skip near-white (background) pixels */
const BACKGROUND_THRESHOLD = 240;

/** Minimum OD magnitude to consider a pixel as tissue */
const MIN_OD_MAGNITUDE = 0.15;

/**
 * Extract optical density samples from a pixel buffer.
 * Samples a fraction of non-background pixels for efficiency.
 *
 * @param pixels - RGBA pixel buffer (Uint8ClampedArray)
 * @param sampleFraction - Fraction of pixels to sample (0-1)
 * @param maxSamples - Maximum number of samples to collect
 * @returns Array of OD vectors [odR, odG, odB]
 */
function extractOdFromPixels(
  pixels: Uint8ClampedArray,
  sampleFraction: number = 0.1,
  maxSamples: number = 20000
): number[][] {
  const samples: number[][] = [];
  const numPixels = pixels.length / 4;
  const step = Math.max(1, Math.floor(1 / sampleFraction));

  for (let i = 0; i < numPixels && samples.length < maxSamples; i += step) {
    const offset = i * 4;
    const r = pixels[offset];
    const g = pixels[offset + 1];
    const b = pixels[offset + 2];
    // Skip alpha

    // Skip near-white background pixels
    if (r > BACKGROUND_THRESHOLD && g > BACKGROUND_THRESHOLD && b > BACKGROUND_THRESHOLD) {
      continue;
    }

    const od = rgbToOd(r, g, b);

    // Skip pixels with very low OD (still background-like)
    const odMagnitude = Math.sqrt(od[0] * od[0] + od[1] * od[1] + od[2] * od[2]);
    if (odMagnitude < MIN_OD_MAGNITUDE) {
      continue;
    }

    samples.push(od);
  }

  return samples;
}

// ============================================================================
// Linear Algebra Helpers
// ============================================================================

/**
 * Compute the mean of each column in a 2D array.
 */
function columnMean(data: number[][]): number[] {
  if (data.length === 0) return [];
  const numCols = data[0].length;
  const means = new Array(numCols).fill(0);

  for (const row of data) {
    for (let j = 0; j < numCols; j++) {
      means[j] += row[j];
    }
  }

  for (let j = 0; j < numCols; j++) {
    means[j] /= data.length;
  }

  return means;
}

/**
 * Compute the standard deviation of each column in a 2D array.
 */
function columnStd(data: number[][], means: number[]): number[] {
  if (data.length === 0) return [];
  const numCols = data[0].length;
  const stds = new Array(numCols).fill(0);

  for (const row of data) {
    for (let j = 0; j < numCols; j++) {
      const diff = row[j] - means[j];
      stds[j] += diff * diff;
    }
  }

  for (let j = 0; j < numCols; j++) {
    stds[j] = Math.sqrt(stds[j] / data.length);
    // Prevent division by zero
    if (stds[j] < 1e-6) stds[j] = 1e-6;
  }

  return stds;
}

/**
 * Compute the percentile of each column in a 2D array.
 */
function columnPercentile(data: number[][], percentile: number): number[] {
  if (data.length === 0) return [];
  const numCols = data[0].length;
  const result: number[] = [];
  
  for (let j = 0; j < numCols; j++) {
    const column = data.map(row => row[j]).sort((a, b) => a - b);
    const idx = Math.floor(column.length * percentile);
    result.push(column[Math.min(idx, column.length - 1)]);
  }
  
  return result;
}

/**
 * Normalize a 3D vector to unit length.
 */
function normalizeVector(v: number[]): number[] {
  const magnitude = Math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
  if (magnitude < 1e-10) return [1, 0, 0]; // Fallback
  return [v[0] / magnitude, v[1] / magnitude, v[2] / magnitude];
}

/**
 * Compute the 3x3 covariance matrix of OD samples.
 */
function computeCovarianceMatrix(odSamples: number[][]): number[][] {
  const n = odSamples.length;
  if (n === 0) {
    return [
      [1, 0, 0],
      [0, 1, 0],
      [0, 0, 1],
    ];
  }

  // Compute mean
  const mean = [0, 0, 0];
  for (const od of odSamples) {
    mean[0] += od[0];
    mean[1] += od[1];
    mean[2] += od[2];
  }
  mean[0] /= n;
  mean[1] /= n;
  mean[2] /= n;

  // Compute covariance
  const cov = [
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
  ];

  for (const od of odSamples) {
    const d0 = od[0] - mean[0];
    const d1 = od[1] - mean[1];
    const d2 = od[2] - mean[2];

    cov[0][0] += d0 * d0;
    cov[0][1] += d0 * d1;
    cov[0][2] += d0 * d2;
    cov[1][1] += d1 * d1;
    cov[1][2] += d1 * d2;
    cov[2][2] += d2 * d2;
  }

  // Symmetric
  cov[1][0] = cov[0][1];
  cov[2][0] = cov[0][2];
  cov[2][1] = cov[1][2];

  // Normalize
  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      cov[i][j] /= n;
    }
  }

  return cov;
}

/**
 * Simple power iteration to find the dominant eigenvector of a 3x3 symmetric matrix.
 * Returns the eigenvector and eigenvalue.
 */
function powerIteration(
  matrix: number[][],
  numIterations: number = 50
): { eigenvector: number[]; eigenvalue: number } {
  // Start with a random-ish vector
  let v = [0.577, 0.577, 0.577];

  for (let iter = 0; iter < numIterations; iter++) {
    // Multiply by matrix
    const newV = [
      matrix[0][0] * v[0] + matrix[0][1] * v[1] + matrix[0][2] * v[2],
      matrix[1][0] * v[0] + matrix[1][1] * v[1] + matrix[1][2] * v[2],
      matrix[2][0] * v[0] + matrix[2][1] * v[1] + matrix[2][2] * v[2],
    ];

    // Normalize
    const norm = Math.sqrt(newV[0] * newV[0] + newV[1] * newV[1] + newV[2] * newV[2]);
    if (norm < 1e-10) break;
    v = [newV[0] / norm, newV[1] / norm, newV[2] / norm];
  }

  // Compute eigenvalue (Rayleigh quotient)
  const Av = [
    matrix[0][0] * v[0] + matrix[0][1] * v[1] + matrix[0][2] * v[2],
    matrix[1][0] * v[0] + matrix[1][1] * v[1] + matrix[1][2] * v[2],
    matrix[2][0] * v[0] + matrix[2][1] * v[1] + matrix[2][2] * v[2],
  ];
  const eigenvalue = v[0] * Av[0] + v[1] * Av[1] + v[2] * Av[2];

  return { eigenvector: v, eigenvalue };
}

/**
 * Deflate matrix by removing contribution of eigenvector.
 * M' = M - λ * v * v^T
 */
function deflateMatrix(matrix: number[][], eigenvector: number[], eigenvalue: number): number[][] {
  const result: number[][] = [
    [...matrix[0]],
    [...matrix[1]],
    [...matrix[2]],
  ];

  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      result[i][j] -= eigenvalue * eigenvector[i] * eigenvector[j];
    }
  }

  return result;
}

/**
 * Compute the pseudo-inverse of a 3x2 matrix (for solving OD = S * C).
 * Returns a 2x3 matrix such that C = pinv(S) * OD.
 */
function pseudoInverse3x2(S: number[][]): number[][] {
  // S is 3x2, S^T is 2x3
  // pinv(S) = (S^T * S)^(-1) * S^T

  // Compute S^T * S (2x2 matrix)
  const StS = [
    [
      S[0][0] * S[0][0] + S[1][0] * S[1][0] + S[2][0] * S[2][0],
      S[0][0] * S[0][1] + S[1][0] * S[1][1] + S[2][0] * S[2][1],
    ],
    [
      S[0][1] * S[0][0] + S[1][1] * S[1][0] + S[2][1] * S[2][0],
      S[0][1] * S[0][1] + S[1][1] * S[1][1] + S[2][1] * S[2][1],
    ],
  ];

  // Invert 2x2 matrix
  const det = StS[0][0] * StS[1][1] - StS[0][1] * StS[1][0];
  if (Math.abs(det) < 1e-10) {
    // Singular matrix, return identity-like result
    return [
      [1, 0, 0],
      [0, 1, 0],
    ];
  }

  const invStS = [
    [StS[1][1] / det, -StS[0][1] / det],
    [-StS[1][0] / det, StS[0][0] / det],
  ];

  // Compute (S^T * S)^(-1) * S^T (2x3 matrix)
  const pinv: number[][] = [
    [
      invStS[0][0] * S[0][0] + invStS[0][1] * S[0][1],
      invStS[0][0] * S[1][0] + invStS[0][1] * S[1][1],
      invStS[0][0] * S[2][0] + invStS[0][1] * S[2][1],
    ],
    [
      invStS[1][0] * S[0][0] + invStS[1][1] * S[0][1],
      invStS[1][0] * S[1][0] + invStS[1][1] * S[1][1],
      invStS[1][0] * S[2][0] + invStS[1][1] * S[2][1],
    ],
  ];

  return pinv;
}

// ============================================================================
// Macenko Stain Estimation
// ============================================================================

/**
 * Estimate stain matrix using Macenko's SVD-based method.
 *
 * 1. Compute covariance of OD samples
 * 2. Find two dominant eigenvectors (principal plane)
 * 3. Project OD onto this plane and find angular extremes
 * 4. These extremes are the stain vectors
 */
function estimateMacenkoStainMatrix(odSamples: number[][]): number[][] {
  if (odSamples.length < 10) {
    // Not enough samples, return reference matrix
    return REFERENCE_STAIN_MATRIX_MACENKO.map((row) => [...row]);
  }

  // Step 1: Compute covariance matrix
  const cov = computeCovarianceMatrix(odSamples);

  // Step 2: Find two dominant eigenvectors using power iteration + deflation
  const { eigenvector: v1, eigenvalue: e1 } = powerIteration(cov);
  const deflated = deflateMatrix(cov, v1, e1);
  const { eigenvector: v2 } = powerIteration(deflated);

  // Step 3: Project OD samples onto the plane spanned by v1 and v2
  // Compute angles in this 2D projection
  const angles: number[] = [];
  for (const od of odSamples) {
    const proj1 = od[0] * v1[0] + od[1] * v1[1] + od[2] * v1[2];
    const proj2 = od[0] * v2[0] + od[1] * v2[1] + od[2] * v2[2];
    angles.push(Math.atan2(proj2, proj1));
  }

  // Sort angles to find percentiles
  const sortedAngles = [...angles].sort((a, b) => a - b);
  const lowPercentile = 0.01;
  const highPercentile = 0.99;
  const lowIdx = Math.floor(sortedAngles.length * lowPercentile);
  const highIdx = Math.floor(sortedAngles.length * highPercentile);

  const angle1 = sortedAngles[lowIdx];
  const angle2 = sortedAngles[highIdx];

  // Step 4: Convert angles back to stain vectors in OD space
  const stain1 = normalizeVector([
    Math.cos(angle1) * v1[0] + Math.sin(angle1) * v2[0],
    Math.cos(angle1) * v1[1] + Math.sin(angle1) * v2[1],
    Math.cos(angle1) * v1[2] + Math.sin(angle1) * v2[2],
  ]);

  const stain2 = normalizeVector([
    Math.cos(angle2) * v1[0] + Math.sin(angle2) * v2[0],
    Math.cos(angle2) * v1[1] + Math.sin(angle2) * v2[1],
    Math.cos(angle2) * v1[2] + Math.sin(angle2) * v2[2],
  ]);

  // Order stains: Hematoxylin first, then Eosin
  // Use similarity to reference vectors for more robust ordering
  const refH = [REFERENCE_STAIN_MATRIX_MACENKO[0][0], REFERENCE_STAIN_MATRIX_MACENKO[1][0], REFERENCE_STAIN_MATRIX_MACENKO[2][0]];
  const refE = [REFERENCE_STAIN_MATRIX_MACENKO[0][1], REFERENCE_STAIN_MATRIX_MACENKO[1][1], REFERENCE_STAIN_MATRIX_MACENKO[2][1]];
  
  // Compute dot products (cosine similarity for unit vectors)
  const sim1H = Math.abs(stain1[0] * refH[0] + stain1[1] * refH[1] + stain1[2] * refH[2]);
  const sim1E = Math.abs(stain1[0] * refE[0] + stain1[1] * refE[1] + stain1[2] * refE[2]);
  const sim2H = Math.abs(stain2[0] * refH[0] + stain2[1] * refH[1] + stain2[2] * refH[2]);
  const sim2E = Math.abs(stain2[0] * refE[0] + stain2[1] * refE[1] + stain2[2] * refE[2]);
  
  let hStain: number[];
  let eStain: number[];
  
  // Assign based on which stain matches which reference better
  if (sim1H + sim2E > sim1E + sim2H) {
    // stain1 is H, stain2 is E
    hStain = stain1;
    eStain = stain2;
  } else {
    // stain2 is H, stain1 is E
    hStain = stain2;
    eStain = stain1;
  }

  // Build 3x2 stain matrix [H, E] - ensure positive values
  return [
    [Math.abs(hStain[0]), Math.abs(eStain[0])],
    [Math.abs(hStain[1]), Math.abs(eStain[1])],
    [Math.abs(hStain[2]), Math.abs(eStain[2])],
  ];
}

/**
 * Compute stain concentrations for OD samples given a stain matrix.
 */
function computeConcentrations(odSamples: number[][], stainMatrix: number[][]): number[][] {
  const pinv = pseudoInverse3x2(stainMatrix);
  const concentrations: number[][] = [];

  for (const od of odSamples) {
    const c0 = pinv[0][0] * od[0] + pinv[0][1] * od[1] + pinv[0][2] * od[2];
    const c1 = pinv[1][0] * od[0] + pinv[1][1] * od[1] + pinv[1][2] * od[2];
    concentrations.push([Math.max(0, c0), Math.max(0, c1)]);
  }

  return concentrations;
}

// ============================================================================
// Vahadane NMF-based Stain Estimation
// ============================================================================

/**
 * Non-negative Matrix Factorization for stain separation.
 * Approximates OD ≈ W * H where W is stain matrix and H is concentrations.
 *
 * Uses multiplicative update rules:
 * W ← W * (V * H^T) / (W * H * H^T + ε)
 * H ← H * (W^T * V) / (W^T * W * H + ε)
 *
 * @param odData - OD samples as rows [N x 3]
 * @param numStains - Number of stain components (typically 2)
 * @param numIterations - Number of update iterations
 */
function nmf2(
  odData: number[][],
  numStains: number = 2,
  numIterations: number = 50
): { stainMatrix: number[][]; concentrations: number[][] } {
  const n = odData.length;
  if (n === 0) {
    return {
      stainMatrix: REFERENCE_STAIN_MATRIX_VAHADANE.map((row) => [...row]),
      concentrations: [],
    };
  }

  // Initialize W (3 x numStains) with small random perturbations of reference
  const W: number[][] = [];
  for (let i = 0; i < 3; i++) {
    W.push([]);
    for (let j = 0; j < numStains; j++) {
      const ref = j < REFERENCE_STAIN_MATRIX_VAHADANE[i].length 
        ? REFERENCE_STAIN_MATRIX_VAHADANE[i][j] 
        : 0.5;
      W[i].push(Math.max(0.01, ref + (Math.random() - 0.5) * 0.1));
    }
  }

  // Initialize H (numStains x n) with random values
  const H: number[][] = [];
  for (let i = 0; i < numStains; i++) {
    H.push([]);
    for (let j = 0; j < n; j++) {
      H[i].push(Math.random() * 0.5 + 0.1);
    }
  }

  const eps = 1e-10;

  // Multiplicative update iterations
  for (let iter = 0; iter < numIterations; iter++) {
    // Update H: H ← H * (W^T * V) / (W^T * W * H + ε)
    // W^T is numStains x 3, V is 3 x n (odData transposed), H is numStains x n

    // Compute W^T * W (numStains x numStains)
    const WtW: number[][] = [];
    for (let i = 0; i < numStains; i++) {
      WtW.push([]);
      for (let j = 0; j < numStains; j++) {
        let sum = 0;
        for (let k = 0; k < 3; k++) {
          sum += W[k][i] * W[k][j];
        }
        WtW[i].push(sum);
      }
    }

    for (let j = 0; j < n; j++) {
      // Compute W^T * V[:,j] for this sample
      const WtV = new Array(numStains).fill(0);
      for (let s = 0; s < numStains; s++) {
        for (let k = 0; k < 3; k++) {
          WtV[s] += W[k][s] * odData[j][k];
        }
      }

      // Compute W^T * W * H[:,j]
      const WtWH = new Array(numStains).fill(0);
      for (let s = 0; s < numStains; s++) {
        for (let t = 0; t < numStains; t++) {
          WtWH[s] += WtW[s][t] * H[t][j];
        }
      }

      // Update H[:,j]
      for (let s = 0; s < numStains; s++) {
        H[s][j] = Math.max(eps, H[s][j] * WtV[s] / (WtWH[s] + eps));
      }
    }

    // Update W: W ← W * (V * H^T) / (W * H * H^T + ε)
    // V is 3 x n, H^T is n x numStains, result is 3 x numStains

    // Compute H * H^T (numStains x numStains)
    const HHt: number[][] = [];
    for (let i = 0; i < numStains; i++) {
      HHt.push([]);
      for (let j = 0; j < numStains; j++) {
        let sum = 0;
        for (let k = 0; k < n; k++) {
          sum += H[i][k] * H[j][k];
        }
        HHt[i].push(sum);
      }
    }

    for (let i = 0; i < 3; i++) {
      // Compute V[i,:] * H^T (1 x numStains)
      const VHt = new Array(numStains).fill(0);
      for (let s = 0; s < numStains; s++) {
        for (let k = 0; k < n; k++) {
          VHt[s] += odData[k][i] * H[s][k];
        }
      }

      // Compute W[i,:] * H * H^T (1 x numStains)
      const WHHt = new Array(numStains).fill(0);
      for (let s = 0; s < numStains; s++) {
        for (let t = 0; t < numStains; t++) {
          WHHt[s] += W[i][t] * HHt[t][s];
        }
      }

      // Update W[i,:]
      for (let s = 0; s < numStains; s++) {
        W[i][s] = Math.max(eps, W[i][s] * VHt[s] / (WHHt[s] + eps));
      }
    }
  }

  // Normalize columns of W to unit length
  for (let j = 0; j < numStains; j++) {
    let norm = 0;
    for (let i = 0; i < 3; i++) {
      norm += W[i][j] * W[i][j];
    }
    norm = Math.sqrt(norm);
    if (norm > eps) {
      for (let i = 0; i < 3; i++) {
        W[i][j] /= norm;
      }
      // Scale H correspondingly
      for (let k = 0; k < n; k++) {
        H[j][k] *= norm;
      }
    }
  }

  // Order columns so H is first, E is second (based on similarity to reference)
  const refH = [REFERENCE_STAIN_MATRIX_VAHADANE[0][0], REFERENCE_STAIN_MATRIX_VAHADANE[1][0], REFERENCE_STAIN_MATRIX_VAHADANE[2][0]];
  const col0 = [W[0][0], W[1][0], W[2][0]];
  const col1 = [W[0][1], W[1][1], W[2][1]];
  
  const sim0H = Math.abs(col0[0] * refH[0] + col0[1] * refH[1] + col0[2] * refH[2]);
  const sim1H = Math.abs(col1[0] * refH[0] + col1[1] * refH[1] + col1[2] * refH[2]);
  
  // If column 1 is more similar to H, swap columns
  if (sim1H > sim0H) {
    for (let i = 0; i < 3; i++) {
      const tmp = W[i][0];
      W[i][0] = W[i][1];
      W[i][1] = tmp;
    }
    // Also swap H rows
    const tmpRow = H[0];
    H[0] = H[1];
    H[1] = tmpRow;
  }

  // Convert H to array of concentration pairs
  const concentrations: number[][] = [];
  for (let k = 0; k < n; k++) {
    const conc: number[] = [];
    for (let s = 0; s < numStains; s++) {
      conc.push(H[s][k]);
    }
    concentrations.push(conc);
  }

  return { stainMatrix: W, concentrations };
}

// ============================================================================
// Stain Matrix Validation
// ============================================================================

/**
 * Compute cosine similarity between two vectors.
 */
function cosineSimilarity(a: number[], b: number[]): number {
  let dot = 0;
  let normA = 0;
  let normB = 0;
  for (let i = 0; i < a.length; i++) {
    dot += a[i] * b[i];
    normA += a[i] * a[i];
    normB += b[i] * b[i];
  }
  normA = Math.sqrt(normA);
  normB = Math.sqrt(normB);
  if (normA < 1e-10 || normB < 1e-10) return 0;
  return dot / (normA * normB);
}

/**
 * Extract a column from the stain matrix as a vector.
 */
function getStainColumn(matrix: number[][], col: number): number[] {
  return [matrix[0][col], matrix[1][col], matrix[2][col]];
}

/**
 * Minimum cosine similarity between estimated and reference stain vectors.
 * Below this, the estimation is considered unreliable.
 */
const MIN_STAIN_SIMILARITY = 0.7;

/**
 * Minimum angle (in radians) between H and E stain vectors.
 * If too similar, they're not properly separated.
 */
const MIN_STAIN_SEPARATION_ANGLE = 0.3; // ~17 degrees

interface StainMatrixValidation {
  matrix: number[][];
  swapped: boolean;
  usedFallback: boolean;
}

/**
 * Validate and potentially fix an estimated stain matrix.
 * 
 * Checks:
 * 1. H and E vectors are sufficiently different from each other
 * 2. Estimated vectors are similar enough to reference vectors
 * 3. H and E aren't swapped (based on similarity to reference)
 * 
 * @param estimated - The estimated stain matrix
 * @param reference - The reference stain matrix
 * @returns Validated/corrected matrix and flags indicating what was done
 */
function validateAndFixStainMatrix(
  estimated: number[][],
  reference: number[][]
): StainMatrixValidation {
  const estH = getStainColumn(estimated, 0);
  const estE = getStainColumn(estimated, 1);
  const refH = getStainColumn(reference, 0);
  const refE = getStainColumn(reference, 1);

  // Check if H and E are sufficiently separated
  const heSimilarity = Math.abs(cosineSimilarity(estH, estE));
  const separationAngle = Math.acos(Math.min(1, heSimilarity));
  
  if (separationAngle < MIN_STAIN_SEPARATION_ANGLE) {
    console.debug(`Stain vectors too similar (angle: ${(separationAngle * 180 / Math.PI).toFixed(1)}°), using reference`);
    return {
      matrix: reference.map(row => [...row]),
      swapped: false,
      usedFallback: true,
    };
  }

  // Check similarity to reference vectors
  const estHtoRefH = cosineSimilarity(estH, refH);
  const estEtoRefE = cosineSimilarity(estE, refE);
  const estHtoRefE = cosineSimilarity(estH, refE);
  const estEtoRefH = cosineSimilarity(estE, refH);

  // Check if stains are swapped: estH is more similar to refE and vice versa
  const normalMatch = Math.min(estHtoRefH, estEtoRefE);
  const swappedMatch = Math.min(estHtoRefE, estEtoRefH);

  if (swappedMatch > normalMatch) {
    // Stains are swapped - swap columns
    console.debug(`H/E stains appear swapped (normal: ${normalMatch.toFixed(2)}, swapped: ${swappedMatch.toFixed(2)})`);
    return {
      matrix: [
        [estimated[0][1], estimated[0][0]],
        [estimated[1][1], estimated[1][0]],
        [estimated[2][1], estimated[2][0]],
      ],
      swapped: true,
      usedFallback: false,
    };
  }

  // Check if estimation is too different from reference (anomalous)
  if (normalMatch < MIN_STAIN_SIMILARITY) {
    console.debug(`Stain vectors too different from reference (similarity: ${normalMatch.toFixed(2)}), using reference`);
    return {
      matrix: reference.map(row => [...row]),
      swapped: false,
      usedFallback: true,
    };
  }

  // Estimation looks good
  return {
    matrix: estimated,
    swapped: false,
    usedFallback: false,
  };
}

// ============================================================================
// Parameter Computation and Stability Checking
// ============================================================================

/**
 * Compute normalization parameters from accumulated OD samples.
 * This is the core computation, extracted for reuse in stability checking.
 *
 * @param samples - Accumulated OD samples
 * @param mode - Normalization mode
 * @returns Computed parameters, or null if computation fails
 */
function computeNormalizationParams(
  samples: number[][],
  mode: StainNormalizationMode
): NormalizationParams | null {
  if (samples.length < MIN_SAMPLES_FOR_ESTIMATION) {
    return null;
  }

  let stainMatrix: number[][];
  let concentrations: number[][];
  const refMatrix = mode === 'macenko' ? REFERENCE_STAIN_MATRIX_MACENKO : REFERENCE_STAIN_MATRIX_VAHADANE;

  if (mode === 'macenko') {
    // Macenko: SVD-based estimation
    stainMatrix = estimateMacenkoStainMatrix(samples);
    concentrations = computeConcentrations(samples, stainMatrix);
  } else {
    // Vahadane: NMF-based estimation
    const nmfResult = nmf2(samples, 2, 50);
    stainMatrix = nmfResult.stainMatrix;
    concentrations = nmfResult.concentrations;
  }

  // Validate the estimated stain matrix against reference
  // This catches cases where H/E are swapped or estimation went wrong
  const validationResult = validateAndFixStainMatrix(stainMatrix, refMatrix);
  if (validationResult.usedFallback) {
    console.warn(`Stain matrix estimation produced anomalous results, using reference matrix`);
    stainMatrix = validationResult.matrix;
    // Recompute concentrations with the corrected matrix
    concentrations = computeConcentrations(samples, stainMatrix);
  } else if (validationResult.swapped) {
    console.info(`Stain matrix H/E were swapped, corrected`);
    stainMatrix = validationResult.matrix;
    // Recompute concentrations with the corrected matrix
    concentrations = computeConcentrations(samples, stainMatrix);
  }

  // Compute 99th percentile of concentrations (max stain intensity)
  const maxC = columnPercentile(concentrations, 0.99);

  // Validate maxC - use reference values if computed values are invalid
  const refMaxC = mode === 'macenko' ? REFERENCE_MAX_C_MACENKO : REFERENCE_MAX_C_VAHADANE;
  if (maxC.length < 2 || maxC[0] < 0.1 || maxC[1] < 0.1 || !Number.isFinite(maxC[0]) || !Number.isFinite(maxC[1])) {
    maxC[0] = refMaxC[0];
    maxC[1] = refMaxC[1];
  }

  return {
    stainMatrix,
    maxC,
    mode,
  };
}

/**
 * Check if two sets of normalization parameters are similar enough.
 * Used for stability checking - if params diverge too much, we invalidate cache.
 *
 * @param a - First parameter set
 * @param b - Second parameter set
 * @returns true if parameters are within stability threshold
 */
function areParamsStable(a: NormalizationParams, b: NormalizationParams): boolean {
  // Compare maxC values (most important for color appearance)
  for (let i = 0; i < 2; i++) {
    const valA = a.maxC[i];
    const valB = b.maxC[i];
    const avg = (valA + valB) / 2;
    if (avg > 0) {
      const relDiff = Math.abs(valA - valB) / avg;
      if (relDiff > STABILITY_THRESHOLD) {
        console.debug(`maxC[${i}] diverged: ${valA.toFixed(3)} vs ${valB.toFixed(3)} (${(relDiff * 100).toFixed(1)}%)`);
        return false;
      }
    }
  }

  // Compare stain matrix vectors
  for (let col = 0; col < 2; col++) {
    for (let row = 0; row < 3; row++) {
      const valA = a.stainMatrix[row][col];
      const valB = b.stainMatrix[row][col];
      const avg = (Math.abs(valA) + Math.abs(valB)) / 2;
      if (avg > 0.01) { // Only check non-trivial values
        const relDiff = Math.abs(valA - valB) / avg;
        if (relDiff > STABILITY_THRESHOLD) {
          console.debug(`stainMatrix[${row}][${col}] diverged: ${valA.toFixed(3)} vs ${valB.toFixed(3)} (${(relDiff * 100).toFixed(1)}%)`);
          return false;
        }
      }
    }
  }

  return true;
}

// ============================================================================
// Main API
// ============================================================================

/**
 * Get or compute normalization parameters for a slide.
 * Accumulates samples from multiple tiles for more robust estimation.
 * Results are cached per slide+mode combination.
 * 
 * Includes stability checking: every few tiles, we recompute candidate
 * parameters and compare to cached. If they diverge significantly, we
 * invalidate the cache and re-estimate with more samples.
 *
 * @param slideId - Unique identifier for the slide
 * @param pixels - Representative pixel buffer (RGBA) for parameter estimation
 * @param mode - Normalization mode
 * @returns Computed parameters, or null if still accumulating samples
 */
export function getOrComputeNormalizationParams(
  slideId: string,
  pixels: Uint8ClampedArray,
  mode: StainNormalizationMode
): NormalizationParams | null {
  if (mode === 'none') {
    return null;
  }

  const cacheKey = `${slideId}_${mode}`;

  // Extract OD samples from this tile
  const newSamples = extractOdFromPixels(pixels, 0.1, 5000);
  
  // Get or create accumulated samples for this slide
  let accumulated = sampleAccumulator.get(cacheKey) || [];
  const numTiles = (tilesContributed.get(cacheKey) || 0) + 1;
  tilesContributed.set(cacheKey, numTiles);
  
  // Add new samples (with limit to prevent memory issues)
  if (accumulated.length < MAX_ACCUMULATED_SAMPLES) {
    accumulated = accumulated.concat(newSamples.slice(0, MAX_ACCUMULATED_SAMPLES - accumulated.length));
    sampleAccumulator.set(cacheKey, accumulated);
  }
  
  // Check if we have cached params and should do a stability check
  const cachedParams = normalizationCache.get(cacheKey);
  if (cachedParams) {
    const lastCheck = lastStabilityCheck.get(cacheKey) || 0;
    // Perform stability check every STABILITY_CHECK_INTERVAL tiles
    if (numTiles - lastCheck >= STABILITY_CHECK_INTERVAL && accumulated.length >= MIN_SAMPLES_FOR_ESTIMATION) {
      lastStabilityCheck.set(cacheKey, numTiles);
      const candidateParams = computeNormalizationParams(accumulated, mode);
      if (candidateParams && !areParamsStable(cachedParams, candidateParams)) {
        // Parameters diverged too much - invalidate cache and re-estimate with more samples
        console.warn(`Stain normalization params unstable for ${slideId}, invalidating cache`);
        normalizationCache.delete(cacheKey);
        stableCheckCount.delete(cacheKey);
        // Don't clear accumulated samples - keep building up for better estimate
      } else {
        // Params are stable - track consecutive stable checks
        const stableCount = (stableCheckCount.get(cacheKey) || 0) + 1;
        stableCheckCount.set(cacheKey, stableCount);
        
        // Clear accumulator after params have been stable for a few checks
        if (stableCount >= STABLE_CHECKS_BEFORE_CLEANUP) {
          sampleAccumulator.delete(cacheKey);
          tilesContributed.delete(cacheKey);
          lastStabilityCheck.delete(cacheKey);
          // Keep stableCheckCount to remember we already cleaned up
        }
        return cachedParams;
      }
    } else {
      // Not time for stability check yet, return cached
      return cachedParams;
    }
  }
  
  // Need enough samples from sufficient tiles for robust estimation
  if (accumulated.length < MIN_SAMPLES_FOR_ESTIMATION || numTiles < MIN_TILES_FOR_ESTIMATION) {
    // Not enough samples yet - return null to skip normalization for now
    // The tile will be re-processed once params are available
    return null;
  }
  
  // We have enough samples - compute parameters
  const params = computeNormalizationParams(accumulated, mode);
  if (!params) {
    return null;
  }

  // Cache the computed parameters
  // Keep accumulator for stability checking - only clear when we've reached max samples
  normalizationCache.set(cacheKey, params);
  lastStabilityCheck.set(cacheKey, numTiles);
  
  // Clean up if we've accumulated enough samples
  if (accumulated.length >= MAX_ACCUMULATED_SAMPLES) {
    sampleAccumulator.delete(cacheKey);
    tilesContributed.delete(cacheKey);
    lastStabilityCheck.delete(cacheKey);
  }
  
  return params;
}

/**
 * Apply stain normalization to a tile's pixel buffer in-place.
 *
 * The normalization process:
 * 1. Convert each pixel RGB → OD
 * 2. Compute stain concentrations: C = pinv(S_slide) * OD
 * 3. Scale concentrations: C_norm = C / slideMaxC * refMaxC
 * 4. Reconstruct with reference stain matrix: OD_ref = S_ref * C_norm
 * 5. Convert OD_ref → RGB
 *
 * @param pixels - RGBA pixel buffer to modify in-place
 * @param mode - Normalization mode
 * @param params - Pre-computed normalization parameters
 */
export function applyStainNormalizationToTile(
  pixels: Uint8ClampedArray,
  mode: StainNormalizationMode,
  params: NormalizationParams | null
): void {
  if (mode === 'none' || !params) {
    return;
  }

  // Get reference parameters
  const refMatrix = mode === 'macenko'
    ? REFERENCE_STAIN_MATRIX_MACENKO
    : REFERENCE_STAIN_MATRIX_VAHADANE;
  const refMaxC = mode === 'macenko'
    ? REFERENCE_MAX_C_MACENKO
    : REFERENCE_MAX_C_VAHADANE;

  // Precompute pseudo-inverse of slide stain matrix
  const pinv = pseudoInverse3x2(params.stainMatrix);
  
  // Maximum allowed scaling factor to prevent washed-out or over-saturated results
  const MAX_SCALE_FACTOR = 3.0;
  const MIN_SCALE_FACTOR = 0.33;
  
  // Precompute scaling factors (refMaxC / slideMaxC) with clamping
  let scale0 = params.maxC[0] > 1e-6 ? refMaxC[0] / params.maxC[0] : 1;
  let scale1 = params.maxC[1] > 1e-6 ? refMaxC[1] / params.maxC[1] : 1;
  
  // Clamp scaling factors to prevent extreme results
  scale0 = Math.max(MIN_SCALE_FACTOR, Math.min(MAX_SCALE_FACTOR, scale0));
  scale1 = Math.max(MIN_SCALE_FACTOR, Math.min(MAX_SCALE_FACTOR, scale1));

  const numPixels = pixels.length / 4;

  for (let i = 0; i < numPixels; i++) {
    const offset = i * 4;
    const r = pixels[offset];
    const g = pixels[offset + 1];
    const b = pixels[offset + 2];
    // Alpha is preserved

    // Skip near-white background pixels (optimization)
    if (r > BACKGROUND_THRESHOLD && g > BACKGROUND_THRESHOLD && b > BACKGROUND_THRESHOLD) {
      continue;
    }

    // Step 1: RGB → OD
    const od = rgbToOd(r, g, b);

    // Step 2: Compute stain concentrations C = pinv(S_slide) * OD
    const c0 = pinv[0][0] * od[0] + pinv[0][1] * od[1] + pinv[0][2] * od[2];
    const c1 = pinv[1][0] * od[0] + pinv[1][1] * od[1] + pinv[1][2] * od[2];

    // Step 3: Scale concentrations to match reference intensity
    // C_norm = C / slideMaxC * refMaxC
    const c0Norm = Math.max(0, c0) * scale0;
    const c1Norm = Math.max(0, c1) * scale1;

    // Step 4: Reconstruct OD using reference stain matrix
    // OD_ref = S_ref * C_norm
    const odRefR = refMatrix[0][0] * c0Norm + refMatrix[0][1] * c1Norm;
    const odRefG = refMatrix[1][0] * c0Norm + refMatrix[1][1] * c1Norm;
    const odRefB = refMatrix[2][0] * c0Norm + refMatrix[2][1] * c1Norm;

    // Step 5: OD → RGB
    const rgb = odToRgb(odRefR, odRefG, odRefB);

    // Write back to pixel buffer
    pixels[offset] = rgb[0];
    pixels[offset + 1] = rgb[1];
    pixels[offset + 2] = rgb[2];
    // Alpha unchanged
  }
}

/**
 * Apply stain normalization to an ImageData object in-place.
 * Convenience wrapper for canvas-based workflows.
 */
export function applyStainNormalizationToImageData(
  imageData: ImageData,
  mode: StainNormalizationMode,
  params: NormalizationParams | null
): void {
  applyStainNormalizationToTile(imageData.data, mode, params);
}

/**
 * Create a normalized ImageBitmap from a source bitmap.
 * Uses OffscreenCanvas for processing if available.
 *
 * @param source - Source ImageBitmap
 * @param mode - Normalization mode
 * @param params - Pre-computed normalization parameters
 * @returns Promise resolving to normalized ImageBitmap
 */
export async function createNormalizedBitmap(
  source: ImageBitmap,
  mode: StainNormalizationMode,
  params: NormalizationParams | null
): Promise<ImageBitmap> {
  if (mode === 'none' || !params) {
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
    applyStainNormalizationToImageData(imageData, mode, params);
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
  applyStainNormalizationToImageData(imageData, mode, params);
  ctx.putImageData(imageData, 0, 0);

  return createImageBitmap(canvas);
}
