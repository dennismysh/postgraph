import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const since = url.searchParams.get('since');
  const grouping = url.searchParams.get('grouping');
  const searchParams = new URLSearchParams();
  if (since) searchParams.set('since', since);
  if (grouping) searchParams.set('grouping', grouping);
  return proxyToBackend('/api/analytics/views', { searchParams });
};
