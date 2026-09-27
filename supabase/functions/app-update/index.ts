/**
 * GET /functions/v1/app-update — the tills' update channel (see
 * `_shared/app-update.ts`). Headers: `x-pos-license`, `x-pos-device-key`.
 */
import { handleUpdate, storageSigner } from '../_shared/app-update.ts';
import { env } from '../_shared/env.ts';

const signUrl = storageSigner();
Deno.serve((request) => handleUpdate(request, { ...env(), signUrl }));
