/**
 * Database access for the edge functions. All logic lives in SQL
 * (supabase/migrations); this is a thin, typed call layer over a direct
 * Postgres connection (`SUPABASE_DB_URL`, transaction pooler), so the same
 * handlers run in Supabase and in the local dev server.
 */
import postgres from 'npm:postgres@3.4.5';

export interface ValidateDeviceArgs {
  clientId: string;
  clientSlug: string;
  maxDevices: number;
  tokenIssuedAt: string;
  tokenId: string;
  fingerprint: string;
  deviceName: string;
}

export interface Db {
  validateDevice(
    args: ValidateDeviceArgs,
  ): Promise<{ status: string; server_time: Date; reason: string | null }>;
  push(
    clientId: string,
    fingerprint: string,
    deviceId: string,
    events: unknown[],
  ): Promise<unknown>;
  pull(
    clientId: string,
    fingerprint: string,
    deviceId: string,
    cursor: string | null,
    limit: number,
  ): Promise<unknown>;
}

/** SQLSTATE raised by `_sync_authorize` when a device may not sync. */
export const NOT_AUTHORIZED = '28000';

export function connect(url: string): Db {
  const sql = postgres(url, { prepare: false, max: 1, idle_timeout: 20 });
  return {
    async validateDevice(a) {
      const [row] = await sql`
        select * from public.validate_device_activation(
          ${a.clientId}, ${a.clientSlug}, ${a.maxDevices}, ${a.tokenIssuedAt},
          ${a.tokenId}, ${a.fingerprint}, ${a.deviceName})`;
      return row as { status: string; server_time: Date; reason: string | null };
    },
    async push(clientId, fingerprint, deviceId, events) {
      const [row] = await sql`
        select public.sync_push(${clientId}, ${fingerprint}, ${deviceId}, ${sql.json(events as never)}) as result`;
      return row?.['result'];
    },
    async pull(clientId, fingerprint, deviceId, cursor, limit) {
      const [row] = await sql`
        select public.sync_pull(${clientId}, ${fingerprint}, ${deviceId}, ${cursor ?? '0'}::bigint, ${limit}) as result`;
      return row?.['result'];
    },
  };
}

export function sqlState(error: unknown): string | undefined {
  return typeof error === 'object' && error !== null && 'code' in error
    ? String((error as { code: unknown }).code)
    : undefined;
}
