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
 */
import { env } from './_shared/env.ts';
import { handleValidate } from './_shared/license-validate.ts';
import { handlePull, handlePush } from './_shared/sync.ts';

const routes: Record<string, (request: Request) => Promise<Response>> = {
  '/functions/v1/license-validate': (r) => handleValidate(r, env()),
  '/functions/v1/sync-push': (r) => handlePush(r, env()),
  '/functions/v1/sync-pull': (r) => handlePull(r, env()),
};

const port = Number(Deno.env.get('PORT') ?? '54321');
Deno.serve({ port, hostname: '127.0.0.1' }, async (request) => {
  const route = routes[new URL(request.url).pathname];
  const response = route ? await route(request) : new Response('not found', { status: 404 });
  console.log(`${request.method} ${new URL(request.url).pathname} → ${String(response.status)}`);
  return response;
});
