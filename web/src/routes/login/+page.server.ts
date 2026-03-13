import { fail, redirect } from '@sveltejs/kit';
import { DASHBOARD_PASSWORD, SESSION_SECRET } from '$env/static/private';
import * as crypto from 'node:crypto';
import type { Actions } from './$types';

function createSession(): string {
  const payload = Buffer.from(JSON.stringify({
    authenticated: true,
    expires: Date.now() + 7 * 24 * 60 * 60 * 1000, // 7 days
  })).toString('base64url');

  const signature = crypto
    .createHmac('sha256', SESSION_SECRET)
    .update(payload)
    .digest('base64url');

  return `${payload}.${signature}`;
}

export const actions: Actions = {
  default: async ({ request, cookies }) => {
    const form = await request.formData();
    const password = form.get('password') as string;

    if (!password) {
      return fail(400, { error: 'Password is required' });
    }

    // Constant-time comparison
    const expected = Buffer.from(DASHBOARD_PASSWORD);
    const received = Buffer.from(password);

    if (expected.length !== received.length ||
        !crypto.timingSafeEqual(expected, received)) {
      return fail(401, { error: 'Invalid password' });
    }

    cookies.set('session', createSession(), {
      path: '/',
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'strict',
      maxAge: 7 * 24 * 60 * 60, // 7 days in seconds
    });

    throw redirect(303, '/');
  },
};
