/**
 * POST /functions/v1/sync-push — body `SyncPushRequest`, response `SyncPushResponse`.
 * Headers: `x-pos-license` (token) and `x-pos-device-key` (sync key).
 * Secrets: LICENSE_PUBLIC_KEY_PEM; SUPABASE_DB_URL is provided by the platform.
 */
import { env } from '../_shared/env.ts';
import { handlePush } from '../_shared/sync.ts';

Deno.serve((request) => handlePush(request, env()));
