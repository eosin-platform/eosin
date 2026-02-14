import { json } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const offset = parseInt(url.searchParams.get('offset') ?? '0');
  const limit = parseInt(url.searchParams.get('limit') ?? '50');
  const datasetId = url.searchParams.get('dataset_id');

  if (!datasetId || datasetId.trim().length === 0) {
    return json({ error: 'dataset_id is required' }, { status: 400 });
  }

  const metaEndpoint = env.META_ENDPOINT;
  
  if (!metaEndpoint) {
    return json({ error: 'Server configuration error' }, { status: 500 });
  }

  try {
    const params = new URLSearchParams({
      offset: offset.toString(),
      limit: limit.toString(),
      dataset_id: datasetId,
    });

    const response = await fetch(`${metaEndpoint}/slides?${params.toString()}`);

    if (!response.ok) {
      return json({ error: 'Failed to fetch slides' }, { status: response.status });
    }

    const data = await response.json();
    return json(data);
  } catch (err) {
    console.error('Failed to fetch slides:', err);
    return json({ error: 'Failed to connect to metadata server' }, { status: 500 });
  }
};
