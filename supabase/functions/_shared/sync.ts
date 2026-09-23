/**
 * HTTP layer of the sync protocol (`@pos/shared` sync.ts). Validation of the
 * request envelope happens here; row-level validation and conflict
 * resolution happen in SQL (`sync_push` / `sync_pull`).
 */
import { NOT_AUTHORIZED, sqlState, type Db } from './db.ts';
import { authenticateDevice, DeviceAuthError } from './device-auth.ts';
import type { VerificationKey } from './license.ts';

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const MAX_BODY_BYTES = 4 * 1024 * 1024;
export const MAX_EVENTS = 500;

export interface Deps {
  db: Db;
  key: () => Promise<VerificationKey>;
  now?: () => Date;
}

function json(status: number, body: unknown): Response {
  return Response.json(body, { status });
}

async function readBody(request: Request): Promise<Record<string, unknown> | null> {
  const length = Number(request.headers.get('content-length') ?? '0');
  if (length > MAX_BODY_BYTES) return null;
  const text = await request.text();
  if (text.length > MAX_BODY_BYTES) return null;
  try {
    const value: unknown = JSON.parse(text);
    return typeof value === 'object' && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function isEvent(value: unknown): boolean {
  if (typeof value !== 'object' || value === null) return false;
  const e = value as Record<string, unknown>;
  return (
    typeof e['event_id'] === 'string' &&
    UUID.test(e['event_id']) &&
    typeof e['entity_id'] === 'string' &&
    UUID.test(e['entity_id']) &&
    (e['event_type'] === 'upsert' || e['event_type'] === 'append') &&
    typeof e['entity_type'] === 'string' &&
    /^[a-z_]{1,64}$/.test(e['entity_type']) &&
    typeof e['payload'] === 'object' &&
    e['payload'] !== null &&
    !Array.isArray(e['payload'])
  );
}

async function run(
  request: Request,
  deps: Deps,
  body: (
    claims: { sub: string; fp: string },
    payload: Record<string, unknown>,
  ) => Promise<Response>,
): Promise<Response> {
  if (request.method !== 'POST') return json(405, { error: 'method not allowed' });
  let claims;
  try {
    claims = await authenticateDevice(request, await deps.key(), deps.now?.() ?? new Date());
  } catch (error) {
    if (error instanceof DeviceAuthError) return json(401, { error: error.message });
    throw error;
  }
  const payload = await readBody(request);
  if (
    !payload ||
    payload['protocol_version'] !== 1 ||
    typeof payload['device_id'] !== 'string' ||
    !UUID.test(payload['device_id'])
  ) {
    return json(400, { error: 'invalid request body' });
  }
  try {
    return await body(claims, payload);
  } catch (error) {
    // 403: this device may not sync (not activated / revoked / wrong device id).
    // Anything else is the server's problem: 503 means "try again later",
    // which the till treats as offline — never as data loss.
    if (sqlState(error) === NOT_AUTHORIZED) {
      return json(403, { error: error instanceof Error ? error.message : 'not authorized' });
    }
    console.error(error);
    return json(503, { error: 'sync temporarily unavailable' });
  }
}

export function handlePush(request: Request, deps: Deps): Promise<Response> {
  return run(request, deps, async (claims, payload) => {
    const events = payload['events'];
    if (
      !Array.isArray(events) ||
      events.length === 0 ||
      events.length > MAX_EVENTS ||
      !events.every(isEvent)
    ) {
      return json(400, { error: `events must be 1–${String(MAX_EVENTS)} well-formed sync events` });
    }
    const result = await deps.db.push(
      claims.sub,
      claims.fp,
      payload['device_id'] as string,
      events,
    );
    return json(200, result);
  });
}

export function handlePull(request: Request, deps: Deps): Promise<Response> {
  return run(request, deps, async (claims, payload) => {
    const cursor = payload['cursor'];
    const limit = payload['limit'];
    if (!(cursor === null || (typeof cursor === 'string' && /^\d{1,19}$/.test(cursor)))) {
      return json(400, { error: 'cursor must be null or a decimal string' });
    }
    if (typeof limit !== 'number' || !Number.isInteger(limit) || limit < 1 || limit > 1000) {
      return json(400, { error: 'limit must be an integer 1–1000' });
    }
    const result = await deps.db.pull(
      claims.sub,
      claims.fp,
      payload['device_id'] as string,
      cursor,
      limit,
    );
    return json(200, result);
  });
}
