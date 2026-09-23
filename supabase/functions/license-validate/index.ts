/**
 * POST /functions/v1/license-validate — see `_shared/license-validate.ts`.
 * Secrets: LICENSE_PUBLIC_KEY_PEM; SUPABASE_DB_URL is provided by the platform.
 */
import { env } from '../_shared/env.ts';
import { handleValidate } from '../_shared/license-validate.ts';

Deno.serve((request) => handleValidate(request, env()));
