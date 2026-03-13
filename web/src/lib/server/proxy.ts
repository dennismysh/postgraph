import { API_URL, API_KEY } from '$env/static/private';

/**
 * Proxy a request to the backend API, safely handling non-JSON error responses.
 * Returns a proper JSON Response even when the backend returns HTML or an empty body.
 */
export async function proxyToBackend(
  path: string,
  options: { method?: string; searchParams?: URLSearchParams } = {},
): Promise<Response> {
  const url = new URL(`${API_URL}${path}`);
  if (options.searchParams) {
    for (const [key, value] of options.searchParams) {
      url.searchParams.set(key, value);
    }
  }

  let res: globalThis.Response;
  try {
    res = await fetch(url.toString(), {
      method: options.method ?? 'GET',
      headers: { 'Authorization': `Bearer ${API_KEY}` },
    });
  } catch (e) {
    return jsonResponse({ error: `Backend unreachable: ${e instanceof Error ? e.message : 'unknown'}` }, 502);
  }

  // Try to parse as JSON; if the body isn't valid JSON (e.g. HTML error page),
  // return a structured JSON error instead of crashing.
  const text = await res.text();
  if (!res.ok) {
    // Attempt to parse error JSON from backend
    try {
      const data = JSON.parse(text);
      return jsonResponse(data, res.status);
    } catch {
      return jsonResponse({ error: `Backend returned ${res.status}: ${text.slice(0, 200)}` }, res.status);
    }
  }

  try {
    const data = JSON.parse(text);
    return jsonResponse(data, res.status);
  } catch {
    return jsonResponse({ error: `Invalid JSON from backend: ${text.slice(0, 200)}` }, 502);
  }
}

function jsonResponse(data: unknown, status: number): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}
