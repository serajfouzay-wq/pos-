/**
 * GET /functions/v1/app-update?current_version=0.1.3&target=windows&arch=x86_64
 *
 * The Tauri updater's endpoint for a till. Authenticated like sync (license
 * token + device key); answers 204 when the till is up to date, else the
 * Tauri update manifest with a short-lived download URL for the signed
 * installer. The till verifies the signature with the public key compiled
 * into it, so the download location does not have to be trusted.
 */
import { NOT_AUTHORIZED, sqlState, type Db } from './db.ts';
import { authenticateDevice, DeviceAuthError } from './device-auth.ts';
import type { VerificationKey } from './license.ts';

const VERSION = /^\d{1,5}\.\d{1,5}\.\d{1,9}$/;
const TARGETS = new Set(['windows-x86_64']);
/** Minutes a download link stays valid. */
export const LINK_MINUTES = 60;

export interface UpdateDeps {
  db: Db;
  key: () => Promise<VerificationKey>;
  /** A URL the till can download `storage_path` from. */
  signUrl: (storagePath: string, expiresInSeconds: number) => Promise<string>;
  now?: () => Date;
}

function json(status: number, body: unknown): Response {
  return Response.json(body, { status });
}

export async function handleUpdate(request: Request, deps: UpdateDeps): Promise<Response> {
  if (request.method !== 'GET') return json(405, { error: 'method not allowed' });
  let claims;
  try {
    claims = await authenticateDevice(request, await deps.key(), deps.now?.() ?? new Date());
  } catch (error) {
    if (error instanceof DeviceAuthError) return json(401, { error: error.message });
    throw error;
  }
  const params = new URL(request.url).searchParams;
  const current = params.get('current_version') ?? '';
  const target = `${params.get('target') ?? ''}-${params.get('arch') ?? ''}`;
  if (!VERSION.test(current))
    return json(400, { error: 'current_version must be MAJOR.MINOR.PATCH' });
  if (!TARGETS.has(target)) return json(400, { error: `no releases for ${target}` });
  try {
    const release = await deps.db.updateCheck(claims.sub, claims.fp, current, target);
    if (!release) return new Response(null, { status: 204 });
    return json(200, {
      version: release.version,
      notes: release.notes,
      pub_date: new Date(release.published_at).toISOString(),
      url: await deps.signUrl(release.storage_path, LINK_MINUTES * 60),
      signature: release.signature,
    });
  } catch (error) {
    if (sqlState(error) === NOT_AUTHORIZED) {
      return json(403, { error: error instanceof Error ? error.message : 'not authorized' });
    }
    console.error(error);
    return json(503, { error: 'updates temporarily unavailable' });
  }
}

/**
 * Signed URLs from Supabase Storage (the private `releases` bucket), or —
 * for the local dev server — `RELEASES_BASE_URL/<path>`.
 */
export function storageSigner(): (path: string, expires: number) => Promise<string> {
  const base = Deno.env.get('RELEASES_BASE_URL');
  if (base) return (path) => Promise.resolve(`${base.replace(/\/$/, '')}/${path}`);
  const url = Deno.env.get('SUPABASE_URL');
  const serviceKey = Deno.env.get('SUPABASE_SERVICE_ROLE_KEY');
  if (!url || !serviceKey)
    throw new Error('SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY must be set');
  return async (path, expiresIn) => {
    const encoded = path.split('/').map(encodeURIComponent).join('/');
    const response = await fetch(`${url}/storage/v1/object/sign/releases/${encoded}`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${serviceKey}`,
        apikey: serviceKey,
        'content-type': 'application/json',
      },
      body: JSON.stringify({ expiresIn }),
    });
    if (!response.ok) throw new Error(`storage sign failed: HTTP ${String(response.status)}`);
    const body = (await response.json()) as { signedURL?: string };
    if (!body.signedURL) throw new Error('storage sign returned no URL');
    return `${url}/storage/v1${body.signedURL}`;
  };
}
