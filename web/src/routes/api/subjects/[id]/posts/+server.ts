import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ params, url }) => {
  const intent = url.searchParams.get('intent');
  const searchParams = new URLSearchParams();
  if (intent) searchParams.set('intent', intent);
  return proxyToBackend(`/api/subjects/${params.id}/posts`, { searchParams });
};
