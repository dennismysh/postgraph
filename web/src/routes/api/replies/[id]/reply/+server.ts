import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ params, request }) => {
  const body = await request.text();
  return proxyToBackend(`/api/replies/${params.id}/reply`, { method: 'POST', body });
};
