import { env } from '$env/dynamic/private';

const TILE_SIZE = 512;

/**
 * Compute number of mip levels for an image pyramid.
 */
function computeLevels(width: number, height: number): number {
  const maxDim = Math.max(width, height);
  return Math.ceil(Math.log2(maxDim / TILE_SIZE)) + 1;
}

/**
 * Parse a UUID string and validate format.
 */
function isValidUuid(id: string): boolean {
  const uuidRegex = /^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$/i;
  return uuidRegex.test(id);
}

// ============================================================================
// Session State Types (matches urlSync.ts)
// ============================================================================

type StainEnhancementMode = 'none' | 'gram' | 'afb' | 'gms';
type StainNormalization = 'none' | 'macenko' | 'vahadane';

interface TabState {
  s: string;
  l?: string;
  w: number;
  h: number;
  v?: [number, number, number];
}

interface PaneState {
  t: TabState[];
  a: number;
}

interface SessionState {
  p: PaneState[];
  f: number;
  r: number;
  i?: {
    e?: StainEnhancementMode;
    n?: StainNormalization;
    s?: number;
    g?: number;
    b?: number;
    c?: number;
  };
}

/**
 * Decode session state from URL-safe base64.
 */
function decodeSessionState(encoded: string): SessionState | null {
  try {
    let base64 = encoded
      .replace(/-/g, '+')
      .replace(/_/g, '/');
    while (base64.length % 4 !== 0) {
      base64 += '=';
    }
    const json = atob(base64);
    return JSON.parse(json) as SessionState;
  } catch {
    return null;
  }
}

/**
 * Validate that a session state has the required structure.
 */
function validateSessionState(state: unknown): state is SessionState {
  if (!state || typeof state !== 'object') return false;
  const s = state as Record<string, unknown>;
  if (!Array.isArray(s.p) || s.p.length === 0) return false;
  if (typeof s.f !== 'number' || typeof s.r !== 'number') return false;
  
  for (const pane of s.p) {
    if (!pane || typeof pane !== 'object') return false;
    const p = pane as Record<string, unknown>;
    if (!Array.isArray(p.t) || typeof p.a !== 'number') return false;
    for (const tab of p.t) {
      if (!tab || typeof tab !== 'object') return false;
      const t = tab as Record<string, unknown>;
      if (typeof t.s !== 'string' || typeof t.w !== 'number' || typeof t.h !== 'number') return false;
      if (!isValidUuid(t.s)) return false;
    }
  }
  return true;
}

export interface SlideInfo {
  id: string;
  width: number;
  height: number;
  levels: number;
  filename: string;
  /** Optional viewport from permalink */
  viewport: { x: number; y: number; zoom: number } | null;
  /** Optional stain enhancement from permalink */
  stainEnhancement: 'none' | 'gram' | 'afb' | 'gms' | null;
  /** Optional stain normalization from permalink */
  stainNormalization: 'none' | 'macenko' | 'vahadane' | null;
  /** Optional sharpening intensity from permalink (0-100) */
  sharpeningIntensity: number | null;
  /** Optional gamma from permalink (0.1-3.0) */
  gamma: number | null;
  /** Optional brightness from permalink (-100 to 100) */
  brightness: number | null;
  /** Optional contrast from permalink (-100 to 100) */
  contrast: number | null;
}

/**
 * Parse viewport parameters (x, y, zoom) from the URL if present.
 */
function parseViewport(url: URL): { x: number; y: number; zoom: number } | null {
  const xStr = url.searchParams.get('x');
  const yStr = url.searchParams.get('y');
  const zoomStr = url.searchParams.get('zoom');
  if (!xStr || !yStr || !zoomStr) return null;
  const x = parseFloat(xStr);
  const y = parseFloat(yStr);
  const zoom = parseFloat(zoomStr);
  if (!isFinite(x) || !isFinite(y) || !isFinite(zoom) || zoom <= 0) return null;
  return { x, y, zoom };
}

/**
 * Parse stain enhancement mode from URL if present and valid.
 */
function parseStainEnhancement(url: URL): 'none' | 'gram' | 'afb' | 'gms' | null {
  const value = url.searchParams.get('enhance');
  if (value === 'none' || value === 'gram' || value === 'afb' || value === 'gms') {
    return value;
  }
  return null;
}

/**
 * Parse stain normalization mode from URL if present and valid.
 */
function parseStainNormalization(url: URL): 'none' | 'macenko' | 'vahadane' | null {
  const value = url.searchParams.get('normalize');
  if (value === 'none' || value === 'macenko' || value === 'vahadane') {
    return value;
  }
  return null;
}

/**
 * Parse sharpening intensity from URL if present and valid (0-100).
 */
function parseSharpeningIntensity(url: URL): number | null {
  const value = url.searchParams.get('sharpen');
  if (value === null) return null;
  const intensity = parseInt(value, 10);
  if (!isFinite(intensity) || intensity < 0 || intensity > 100) return null;
  return intensity;
}

/**
 * Parse gamma from URL if present and valid (0.1-3.0).
 */
function parseGamma(url: URL): number | null {
  const value = url.searchParams.get('gamma');
  if (value === null) return null;
  const gamma = parseFloat(value);
  if (!isFinite(gamma) || gamma < 0.1 || gamma > 3.0) return null;
  return gamma;
}

/**
 * Parse brightness from URL if present and valid (-100 to 100).
 */
function parseBrightness(url: URL): number | null {
  const value = url.searchParams.get('brightness');
  if (value === null) return null;
  const brightness = parseFloat(value);
  if (!isFinite(brightness) || brightness < -100 || brightness > 100) return null;
  return brightness;
}

/**
 * Parse contrast from URL if present and valid (-100 to 100).
 */
function parseContrast(url: URL): number | null {
  const value = url.searchParams.get('contrast');
  if (value === null) return null;
  const contrast = parseFloat(value);
  if (!isFinite(contrast) || contrast < -100 || contrast > 100) return null;
  return contrast;
}

/** Parsed session info for the ?v= parameter */
export interface ParsedSession {
  state: SessionState;
  imageSettings: {
    stainEnhancement: StainEnhancementMode | null;
    stainNormalization: StainNormalization | null;
    sharpeningIntensity: number | null;
    gamma: number | null;
    brightness: number | null;
    contrast: number | null;
  };
}

export const load = async ({ url }: { url: URL }) => {
  const metaEndpoint = env.META_ENDPOINT;
  
  // Check for session state first (?v= parameter)
  const encodedSession = url.searchParams.get('v');
  if (encodedSession) {
    const sessionState = decodeSessionState(encodedSession);
    if (sessionState && validateSessionState(sessionState)) {
      // Extract image settings from session
      const imageSettings = {
        stainEnhancement: sessionState.i?.e ?? null,
        stainNormalization: sessionState.i?.n ?? null,
        sharpeningIntensity: sessionState.i?.s ?? null,
        gamma: sessionState.i?.g ?? null,
        brightness: sessionState.i?.b ?? null,
        contrast: sessionState.i?.c ?? null,
      };
      
      return { 
        slide: null, 
        error: null, 
        session: { state: sessionState, imageSettings } as ParsedSession 
      };
    } else {
      return { slide: null, error: 'Invalid session state in URL', session: null };
    }
  }
  
  // Fall back to single slide mode (?slide= parameter)
  const id = url.searchParams.get('slide');

  if (!id) {
    return { slide: null, error: null, session: null };
  }

  if (!isValidUuid(id)) {
    return { slide: null, error: 'Invalid slide ID format', session: null };
  }

  if (!metaEndpoint) {
    console.error('META_ENDPOINT environment variable is not set');
    return { slide: null, error: 'Server configuration error', session: null };
  }

  try {
    const response = await fetch(`${metaEndpoint}/slides/${id}`);

    if (response.status === 404) {
      return { slide: null, error: 'Slide not found', session: null };
    }

    if (!response.ok) {
      console.error(`Meta server returned ${response.status}: ${await response.text()}`);
      return { slide: null, error: 'Failed to fetch slide information', session: null };
    }

    const data = await response.json();

    const slide: SlideInfo = {
      id: data.id,
      width: data.width,
      height: data.height,
      levels: computeLevels(data.width, data.height),
      filename: data.filename || data.id.slice(0, 8),
      viewport: parseViewport(url),
      stainEnhancement: parseStainEnhancement(url),
      stainNormalization: parseStainNormalization(url),
      sharpeningIntensity: parseSharpeningIntensity(url),
      gamma: parseGamma(url),
      brightness: parseBrightness(url),
      contrast: parseContrast(url),
    };

    return { slide, error: null, session: null };
  } catch (err) {
    console.error('Failed to fetch slide from meta server:', err);
    return { slide: null, error: 'Failed to connect to metadata server', session: null };
  }
};
