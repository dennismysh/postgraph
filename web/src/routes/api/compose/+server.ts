import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const searchParams = url.searchParams;
  return proxyToBackend('/api/compose', { searchParams });
};

export const POST: RequestHandler = async ({ request }) => {
  const body = await request.text();
  return proxyToBackend('/api/compose', { method: 'POST', body });
};
