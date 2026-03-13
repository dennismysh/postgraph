import { API_URL } from '$env/static/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  let res: globalThis.Response;
  try {
    res = await fetch(`${API_URL}/health/detail`);
  } catch (e) {
    return new Response(JSON.stringify({ error: `Backend unreachable: ${e instanceof Error ? e.message : 'unknown'}` }), {
      status: 502,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  const text = await res.text();
  try {
    const data = JSON.parse(text);
    return new Response(JSON.stringify(data), {
      headers: { 'Content-Type': 'application/json' },
      status: res.status,
    });
  } catch {
    return new Response(JSON.stringify({ error: `Invalid response: ${text.slice(0, 200)}` }), {
      status: 502,
      headers: { 'Content-Type': 'application/json' },
    });
  }
};
