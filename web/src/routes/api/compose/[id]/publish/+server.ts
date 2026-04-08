import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ params }) => {
  return proxyToBackend(`/api/compose/${params.id}/publish`, { method: 'POST' });
};
