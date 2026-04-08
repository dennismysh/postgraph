import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ params }) => {
  return proxyToBackend(`/api/compose/${params.id}`);
};

export const PUT: RequestHandler = async ({ params, request }) => {
  const body = await request.text();
  return proxyToBackend(`/api/compose/${params.id}`, { method: 'PUT', body });
};

export const DELETE: RequestHandler = async ({ params }) => {
  return proxyToBackend(`/api/compose/${params.id}`, { method: 'DELETE' });
};
