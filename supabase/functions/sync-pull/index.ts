/**
 * POST /functions/v1/sync-pull — body `SyncPullRequest`, response `SyncPullResponse`.
 * Headers: `x-pos-license` (token) and `x-pos-device-key` (sync key).
 */
import { env } from '../_shared/env.ts';
import { handlePull } from '../_shared/sync.ts';

Deno.serve((request) => handlePull(request, env()));
