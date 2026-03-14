import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  return proxyToBackend('/api/categorize', { method: 'POST' });
};
