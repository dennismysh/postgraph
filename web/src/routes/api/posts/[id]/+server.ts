import { API_URL, API_KEY } from '$env/static/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ params }) => {
  const res = await fetch(`${API_URL}/api/posts/${params.id}`, {
    headers: { 'Authorization': `Bearer ${API_KEY}` },
  });
  const data = await res.json();
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: res.status,
  });
};
