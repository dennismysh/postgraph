import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const intent = url.searchParams.get('intent');
  const days = url.searchParams.get('days');
  const searchParams = new URLSearchParams();
  if (intent) searchParams.set('intent', intent);
  if (days) searchParams.set('days', days);
  return proxyToBackend('/api/graph', { searchParams });
};
