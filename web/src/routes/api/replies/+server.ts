import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const searchParams = url.searchParams;
  return proxyToBackend('/api/replies', { searchParams });
};
