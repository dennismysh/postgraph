import { redirect, type Handle } from '@sveltejs/kit';
import { SESSION_SECRET } from '$env/static/private';
import * as crypto from 'node:crypto';

function verifySession(cookieValue: string): boolean {
  try {
    const [payload, signature] = cookieValue.split('.');
    if (!payload || !signature) return false;

    const expectedSig = crypto
      .createHmac('sha256', SESSION_SECRET)
      .update(payload)
      .digest('base64url');

    if (!crypto.timingSafeEqual(Buffer.from(signature), Buffer.from(expectedSig))) {
      return false;
    }

    const data = JSON.parse(Buffer.from(payload, 'base64url').toString());
    if (!data.authenticated) return false;
    if (data.expires && Date.now() > data.expires) return false;

    return true;
  } catch {
    return false;
  }
}

export const handle: Handle = async ({ event, resolve }) => {
  const path = event.url.pathname;

  // Allow login page through without auth
  if (path === '/login' || path.startsWith('/login/')) {
    return resolve(event);
  }

  const session = event.cookies.get('session');
  if (!session || !verifySession(session)) {
    throw redirect(303, '/login');
  }

  return resolve(event);
};
