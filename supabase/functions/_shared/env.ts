import { connect, type Db } from './db.ts';
import { importVerificationKey, type VerificationKey } from './license.ts';

/** Lazily-initialised dependencies shared by every function in one isolate. */
let db: Db | undefined;
let key: Promise<VerificationKey> | undefined;

export function env(): { db: Db; key: () => Promise<VerificationKey> } {
  const url = Deno.env.get('SUPABASE_DB_URL');
  const pem = Deno.env.get('LICENSE_PUBLIC_KEY_PEM');
  if (!url || !pem) throw new Error('SUPABASE_DB_URL and LICENSE_PUBLIC_KEY_PEM must be set');
  db ??= connect(url);
  return { db, key: () => (key ??= importVerificationKey(pem)) };
}
