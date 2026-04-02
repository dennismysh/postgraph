import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const since = url.searchParams.get('since');
  const searchParams = new URLSearchParams();
  if (since) searchParams.set('since', since);
  return proxyToBackend('/api/analytics/engagement/daily-deltas', { searchParams });
};
