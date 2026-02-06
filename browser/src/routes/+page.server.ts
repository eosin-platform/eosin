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

export interface SlideInfo {
  id: string;
  width: number;
  height: number;
  levels: number;
  filename: string;
  /** Optional viewport from permalink */
  viewport: { x: number; y: number; zoom: number } | null;
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

export const load = async ({ url }: { url: URL }) => {
  const id = url.searchParams.get('slide');

  if (!id) {
    return { slide: null, error: null };
  }

  if (!isValidUuid(id)) {
    return { slide: null, error: 'Invalid slide ID format' };
  }

  const metaEndpoint = env.META_ENDPOINT;
  if (!metaEndpoint) {
    console.error('META_ENDPOINT environment variable is not set');
    return { slide: null, error: 'Server configuration error' };
  }

  try {
    const response = await fetch(`${metaEndpoint}/slides/${id}`);

    if (response.status === 404) {
      return { slide: null, error: 'Slide not found' };
    }

    if (!response.ok) {
      console.error(`Meta server returned ${response.status}: ${await response.text()}`);
      return { slide: null, error: 'Failed to fetch slide information' };
    }

    const data = await response.json();

    const slide: SlideInfo = {
      id: data.id,
      width: data.width,
      height: data.height,
      levels: computeLevels(data.width, data.height),
      filename: data.filename || data.id.slice(0, 8),
      viewport: parseViewport(url),
    };

    return { slide, error: null };
  } catch (err) {
    console.error('Failed to fetch slide from meta server:', err);
    return { slide: null, error: 'Failed to connect to metadata server' };
  }
};
