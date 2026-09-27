/**
 * Local stand-in for Supabase Edge Functions: serves the real handlers at
 * `/functions/v1/<name>` against a local Postgres. Development and E2E only.
 *
 *   SUPABASE_DB_URL=postgres://postgres@localhost:5432/pos \
 *   LICENSE_PUBLIC_KEY_PEM="$(cat keys/dev/license-dev.public.pem)" \
 *   DENO_NO_PACKAGE_JSON=1 deno run -A supabase/functions/dev-server.ts
 *
 * Point a POS build at it with `cloud.supabase_url = "http://127.0.0.1:54321"`
 * (loopback http is allowed for exactly this).
 *
 * Updates: with `RELEASES_DIR=/path` the installers under it are served at
 * `/releases/<client_id>/<version>/<file>` (`RELEASES_BASE_URL` is then set
 * for the `app-update` handler).
 */
import { handleUpdate } from './_shared/app-update.ts';
import { env } from './_shared/env.ts';
import { handleValidate } from './_shared/license-validate.ts';
import { handlePull, handlePush } from './_shared/sync.ts';

const port = Number(Deno.env.get('PORT') ?? '54321');
const releasesDir = Deno.env.get('RELEASES_DIR');
const releasesBase = `http://127.0.0.1:${String(port)}/releases`;
const signUrl = (path: string) => Promise.resolve(`${releasesBase}/${path}`);

async function serveRelease(pathname: string): Promise<Response> {
  const path = decodeURIComponent(pathname.slice('/releases/'.length));
  if (!releasesDir || path.split('/').some((part) => part === '..' || part === '')) {
    return new Response('not found', { status: 404 });
  }
  try {
    return new Response(await Deno.readFile(`${releasesDir}/${path}`), {
      headers: { 'content-type': 'application/octet-stream' },
    });
  } catch {
    return new Response('not found', { status: 404 });
  }
}

const routes: Record<string, (request: Request) => Promise<Response>> = {
  '/functions/v1/app-update': (r) => handleUpdate(r, { ...env(), signUrl }),
  '/functions/v1/license-validate': (r) => handleValidate(r, env()),
  '/functions/v1/sync-push': (r) => handlePush(r, env()),
  '/functions/v1/sync-pull': (r) => handlePull(r, env()),
};

Deno.serve({ port, hostname: '127.0.0.1' }, async (request) => {
  const { pathname } = new URL(request.url);
  const route = routes[pathname];
  const response = pathname.startsWith('/releases/')
    ? await serveRelease(pathname)
    : route
      ? await route(request)
      : new Response('not found', { status: 404 });
  console.log(`${request.method} ${new URL(request.url).pathname} → ${String(response.status)}`);
  return response;
});
