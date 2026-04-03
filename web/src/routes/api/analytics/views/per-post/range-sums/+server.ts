import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/analytics/views/per-post/range-sums');
};
