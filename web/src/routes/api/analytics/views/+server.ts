import { API_URL, API_KEY } from '$env/static/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const since = url.searchParams.get('since');
  const backendUrl = new URL(`${API_URL}/api/analytics/views`);
  if (since) backendUrl.searchParams.set('since', since);

  const res = await fetch(backendUrl.toString(), {
    headers: { 'Authorization': `Bearer ${API_KEY}` },
  });
  const data = await res.json();
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: res.status,
  });
};
