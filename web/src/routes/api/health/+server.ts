import { API_URL } from '$env/static/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  const res = await fetch(`${API_URL}/health/detail`);
  const data = await res.json();
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: res.status,
  });
};
