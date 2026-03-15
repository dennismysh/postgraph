import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ params }) => {
  return proxyToBackend(`/api/posts/${params.id}/engagement`);
};
