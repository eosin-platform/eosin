import { env } from '$env/dynamic/private';
import { serverRefreshToken } from '$lib/auth/server';
import type { UserCredentials } from '$lib/stores/auth';

const AUTH_COOKIE_NAME = 'eosin_refresh_token';
const AUTH_EXPIRY_COOKIE_NAME = 'eosin_refresh_expiry';
/** Default refresh token expiry: 30 days (in seconds) */
const DEFAULT_REFRESH_EXPIRY_SECONDS = 30 * 24 * 60 * 60;

export interface SlideListItem {
  id: string;
  dataset: string;
  width: number;
  height: number;
  /** Original filename extracted from the S3 key */
  filename: string;
  /** Full size of the original slide file in bytes */
  full_size: number;
  /** Current processing progress in steps */
  progress_steps: number;
  /** Total tiles to process */
  progress_total: number;
}

export interface SlidesResponse {
  offset: number;
  limit: number;
  full_count: number;
  truncated: boolean;
  items: SlideListItem[];
}

export interface DatasetListItem {
  id: string;
  name: string;
  description: string | null;
  credit: string | null;
  created_at: number;
  updated_at: number;
  metadata: unknown | null;
  slide_count: number;
  full_size: number;
}

interface DatasetsResponse {
  offset: number;
  limit: number;
  full_count: number;
  truncated: boolean;
  items: DatasetListItem[];
}

const PAGE_SIZE = 50;
const DATASET_PAGE_SIZE = 1000;

function normalizeEndpoint(endpoint: string): string {
  return endpoint.replace(/\/+$/, '');
}

function derivePublicMetaEndpoint(metaEndpoint: string): string {
  try {
    const url = new URL(metaEndpoint);
    url.port = '3000';
    return normalizeEndpoint(url.toString());
  } catch {
    return normalizeEndpoint(metaEndpoint);
  }
}

async function fetchAllDatasets(metaEndpoint: string, accessToken?: string): Promise<DatasetListItem[]> {
  const items: DatasetListItem[] = [];
  let offset = 0;

  while (true) {
    const response = await fetch(`${metaEndpoint}/dataset?offset=${offset}&limit=${DATASET_PAGE_SIZE}`, {
      headers: accessToken
        ? {
            Authorization: `Bearer ${accessToken}`
          }
        : undefined
    });

    if (!response.ok) {
      throw new Error(`Failed to fetch datasets (${response.status})`);
    }

    const data: DatasetsResponse = await response.json();
    items.push(...data.items);

    if (!data.truncated || data.items.length === 0) {
      break;
    }

    offset += data.items.length;
  }

  return items;
}

export const load = async ({ cookies, request, url }) => {
  // Detect if we're behind HTTPS
  const forwardedProto = request.headers.get('x-forwarded-proto');
  const isSecure = forwardedProto === 'https' || url.protocol === 'https:';
  
  // Read auth cookies using SvelteKit's cookies API
  const refreshToken = cookies.get(AUTH_COOKIE_NAME);
  const refreshExpiryStr = cookies.get(AUTH_EXPIRY_COOKIE_NAME);
  const refreshExpiry = refreshExpiryStr ? parseInt(refreshExpiryStr, 10) : null;

  // List all cookies for debugging
  const allCookies = cookies.getAll();

  let userCredentials: UserCredentials | null = null;
  let newRefreshExpiry: number | null = null;

  // Only attempt refresh if we have a token and it hasn't expired
  if (refreshToken && refreshExpiry && !isNaN(refreshExpiry) && Date.now() < refreshExpiry) {
    console.log('[SSR Auth] Attempting token refresh...');
    userCredentials = await serverRefreshToken(refreshToken);
    console.log('[SSR Auth] Token refresh result:', userCredentials ? 'success' : 'failed');
    
    // Update cookies with the NEW refresh token from the refresh response
    if (userCredentials?.jwt.refresh_token) {
      // refresh_expires_in: 0 means offline token (never expires), use 30 days default
      const expirySeconds = userCredentials.jwt.refresh_expires_in && userCredentials.jwt.refresh_expires_in > 0 
        ? userCredentials.jwt.refresh_expires_in 
        : DEFAULT_REFRESH_EXPIRY_SECONDS;
      newRefreshExpiry = Date.now() + expirySeconds * 1000;
      const expiryDate = new Date(newRefreshExpiry);
      
      cookies.set(AUTH_COOKIE_NAME, userCredentials.jwt.refresh_token, {
        path: '/',
        httpOnly: false,
        sameSite: 'lax',
        secure: isSecure,
        expires: expiryDate
      });
      cookies.set(AUTH_EXPIRY_COOKIE_NAME, newRefreshExpiry.toString(), {
        path: '/',
        httpOnly: false,
        sameSite: 'lax',
        secure: isSecure,
        expires: expiryDate
      });
    }
  }

  const metaEndpoint = env.META_ENDPOINT;
  const publicMetaEndpoint = env.PUBLIC_META_ENDPOINT
    ? normalizeEndpoint(env.PUBLIC_META_ENDPOINT)
    : (metaEndpoint ? derivePublicMetaEndpoint(metaEndpoint) : null);
  
  if (!metaEndpoint) {
    console.error('META_ENDPOINT environment variable is not set');
    return { 
      slides: [], 
      datasets: [],
      selectedDatasetId: null,
      totalCount: 0, 
      hasMore: false,
      pageSize: PAGE_SIZE,
      error: 'Server configuration error',
      auth: userCredentials ? {
        user: userCredentials,
        refreshExpiry: newRefreshExpiry
      } : null
    };
  }

  let datasets: DatasetListItem[] = [];
  try {
    if (!publicMetaEndpoint) {
      throw new Error('PUBLIC_META_ENDPOINT environment variable is not set');
    }
    datasets = await fetchAllDatasets(publicMetaEndpoint, userCredentials?.jwt?.access_token);
  } catch (err) {
    console.error('Failed to fetch datasets from meta server:', err);
  }

  const requestedDatasetId = url.searchParams.get('dataset_id');
  const selectedDatasetId =
    requestedDatasetId && datasets.some((dataset) => dataset.id === requestedDatasetId)
      ? requestedDatasetId
      : datasets.length > 0
        ? datasets[0].id
        : null;

  if (!selectedDatasetId) {
    return {
      slides: [],
      datasets,
      selectedDatasetId: null,
      totalCount: 0,
      hasMore: false,
      pageSize: PAGE_SIZE,
      error: null,
      auth: userCredentials
        ? {
            user: userCredentials,
            refreshExpiry: newRefreshExpiry,
          }
        : null,
    };
  }

  try {
    const response = await fetch(
      `${metaEndpoint}/slides?offset=0&limit=${PAGE_SIZE}&dataset_id=${selectedDatasetId}`
    );

    if (!response.ok) {
      console.error(`Meta server returned ${response.status}: ${await response.text()}`);
      return { 
        slides: [], 
        datasets,
        selectedDatasetId,
        totalCount: 0, 
        hasMore: false,
        pageSize: PAGE_SIZE,
        error: 'Failed to fetch slides',
        auth: userCredentials ? {
          user: userCredentials,
          refreshExpiry: newRefreshExpiry
        } : null
      };
    }

    const data: SlidesResponse = await response.json();

    return { 
      slides: data.items, 
      datasets,
      selectedDatasetId,
      totalCount: data.full_count,
      hasMore: data.offset + data.items.length < data.full_count,
      pageSize: PAGE_SIZE,
      error: null,
      auth: userCredentials ? {
        user: userCredentials,
        refreshExpiry: newRefreshExpiry
      } : null
    };
  } catch (err) {
    console.error('Failed to fetch slides from meta server:', err);
    return { 
      slides: [], 
      datasets,
      selectedDatasetId,
      totalCount: 0, 
      hasMore: false,
      pageSize: PAGE_SIZE,
      error: 'Failed to connect to metadata server',
      auth: userCredentials ? {
        user: userCredentials,
        refreshExpiry: newRefreshExpiry
      } : null
    };
  }
};
