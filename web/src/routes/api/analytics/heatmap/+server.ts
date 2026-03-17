import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const range = url.searchParams.get('range');
  const searchParams = new URLSearchParams();
  if (range) searchParams.set('range', range);
  return proxyToBackend('/api/analytics/heatmap', { searchParams });
};
