import { z } from 'zod';

/**
 * Error codes returned by Rust commands. Serialized by `pos_core::error::IpcError`
 * as `{ code, message }`.
 */
export const IPC_ERROR_CODES = [
  'unauthenticated',
  'forbidden',
  'license_invalid',
  'validation',
  'not_found',
  'conflict',
  'hardware',
  'offline',
  'internal',
] as const;
export const IpcErrorCodeSchema = z.enum(IPC_ERROR_CODES);
export type IpcErrorCode = z.infer<typeof IpcErrorCodeSchema>;

export const IpcErrorPayloadSchema = z.object({
  code: IpcErrorCodeSchema,
  message: z.string(),
});
export type IpcErrorPayload = z.infer<typeof IpcErrorPayloadSchema>;

export class IpcError extends Error {
  override readonly name = 'IpcError';

  constructor(
    readonly code: IpcErrorCode,
    message: string,
    readonly command: string,
    options?: { cause?: unknown },
  ) {
    super(message, options);
  }

  static from(command: string, raw: unknown): IpcError {
    if (raw instanceof IpcError) return raw;
    const parsed = IpcErrorPayloadSchema.safeParse(raw);
    if (parsed.success) return new IpcError(parsed.data.code, parsed.data.message, command);
    const message = raw instanceof Error ? raw.message : String(raw);
    return new IpcError('internal', message, command, { cause: raw });
  }
}
