import { env } from '$env/dynamic/private';

export interface SlideListItem {
  id: string;
  width: number;
  height: number;
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

const PAGE_SIZE = 50;

export const load = async () => {
  const metaEndpoint = env.META_ENDPOINT;
  
  if (!metaEndpoint) {
    console.error('META_ENDPOINT environment variable is not set');
    return { 
      slides: [], 
      totalCount: 0, 
      hasMore: false,
      pageSize: PAGE_SIZE,
      error: 'Server configuration error' 
    };
  }

  try {
    const response = await fetch(`${metaEndpoint}/slides?offset=0&limit=${PAGE_SIZE}`);

    if (!response.ok) {
      console.error(`Meta server returned ${response.status}: ${await response.text()}`);
      return { 
        slides: [], 
        totalCount: 0, 
        hasMore: false,
        pageSize: PAGE_SIZE,
        error: 'Failed to fetch slides' 
      };
    }

    const data: SlidesResponse = await response.json();

    return { 
      slides: data.items, 
      totalCount: data.full_count,
      hasMore: data.offset + data.items.length < data.full_count,
      pageSize: PAGE_SIZE,
      error: null 
    };
  } catch (err) {
    console.error('Failed to fetch slides from meta server:', err);
    return { 
      slides: [], 
      totalCount: 0, 
      hasMore: false,
      pageSize: PAGE_SIZE,
      error: 'Failed to connect to metadata server' 
    };
  }
};
