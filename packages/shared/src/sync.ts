/**
 * Offline sync protocol.
 *
 * Every local mutation is written to `sync_queue` in the SAME SQLite
 * transaction as the mutation itself (outbox pattern). A background worker
 * wakes every {@link SYNC_INTERVAL_MS} and on network reconnect, pushes
 * pending events, then pulls remote changes.
 *
 * Idempotency: `event_id` is the queue row id and the server's dedupe key, so
 * replaying any batch any number of times converges to the same state.
 */
import { z } from 'zod';
import { EntityBaseSchema, NonNegativeIntSchema, TimestampSchema, UuidSchema } from './primitives';

export const SYNC_INTERVAL_MS = 60_000;
export const SYNC_PUSH_BATCH_LIMIT = 500;
export const OFFLINE_GRACE_DAYS = 7;

/**
 * Conflict-resolution strategy per synced table.
 * - `last_write_wins`: mutable records; highest `(updated_at, event_id)` wins.
 * - `append_only`: immutable facts; insert-if-absent, never updated.
 * - `additive_delta`: append-only deltas whose sum is the state (stock, points).
 */
export const CONFLICT_STRATEGIES = ['last_write_wins', 'append_only', 'additive_delta'] as const;
export type ConflictStrategy = (typeof CONFLICT_STRATEGIES)[number];

export const SYNC_ENTITY_STRATEGY = {
  categories: 'last_write_wins',
  products: 'last_write_wins',
  customers: 'last_write_wins',
  users: 'last_write_wins',
  discount_rules: 'last_write_wins',
  shifts: 'last_write_wins',
  suppliers: 'last_write_wins',
  purchase_orders: 'last_write_wins',
  purchase_order_items: 'last_write_wins',
  modifier_groups: 'last_write_wins',
  modifiers: 'last_write_wins',
  product_modifier_groups: 'last_write_wins',
  combos: 'last_write_wins',
  combo_items: 'last_write_wins',
  dining_tables: 'last_write_wins',
  open_orders: 'last_write_wins',
  transactions: 'append_only',
  transaction_items: 'append_only',
  transaction_payments: 'append_only',
  audit_log: 'append_only',
  stock_movements: 'additive_delta',
  loyalty_ledger: 'additive_delta',
} as const satisfies Record<string, ConflictStrategy>;

export type SyncEntityType = keyof typeof SYNC_ENTITY_STRATEGY;
export const SYNC_ENTITY_TYPES = Object.keys(SYNC_ENTITY_STRATEGY) as [
  SyncEntityType,
  ...SyncEntityType[],
];
export const SyncEntityTypeSchema = z.enum(SYNC_ENTITY_TYPES);

/** Aggregates kept as caches of additive-delta tables; never taken from a synced row. */
export const DERIVED_COLUMNS = {
  products: ['stock_on_hand_milli'],
  customers: ['loyalty_points'],
} as const satisfies Partial<Record<SyncEntityType, readonly string[]>>;

/**
 * `upsert` — full-row snapshot for LWW tables (soft deletes are upserts that set `deleted_at`).
 * `append` — insert-once for append-only / additive-delta tables.
 */
export const SYNC_EVENT_TYPES = ['upsert', 'append'] as const;
export const SyncEventTypeSchema = z.enum(SYNC_EVENT_TYPES);
export type SyncEventType = z.infer<typeof SyncEventTypeSchema>;

export function expectedEventType(entity: SyncEntityType): SyncEventType {
  return SYNC_ENTITY_STRATEGY[entity] === 'last_write_wins' ? 'upsert' : 'append';
}

/** Full row snapshot. Validated against the table's own schema on both ends. */
const RowPayloadSchema = z.looseObject(EntityBaseSchema.shape);

/** `sync_queue` table row. */
export const SyncQueueRowSchema = EntityBaseSchema.extend({
  event_type: SyncEventTypeSchema,
  entity_type: SyncEntityTypeSchema,
  entity_id: UuidSchema,
  payload: RowPayloadSchema,
  attempt_count: NonNegativeIntSchema,
  /** Set once the server acknowledged the event. `null` = pending. */
  sent_at: TimestampSchema.nullable(),
  next_attempt_at: TimestampSchema.nullable(),
  last_error: z.string().nullable(),
});
export type SyncQueueRow = z.infer<typeof SyncQueueRowSchema>;

/** Wire envelope for one event (device → cloud). */
export const SyncEventSchema = z
  .object({
    event_id: UuidSchema,
    device_id: UuidSchema,
    event_type: SyncEventTypeSchema,
    entity_type: SyncEntityTypeSchema,
    entity_id: UuidSchema,
    payload: RowPayloadSchema,
    occurred_at: TimestampSchema,
  })
  .refine((event) => event.event_type === expectedEventType(event.entity_type), {
    message: 'event_type does not match the conflict strategy of entity_type',
    path: ['event_type'],
  })
  .refine((event) => event.payload.id === event.entity_id, {
    message: 'payload.id must equal entity_id',
    path: ['payload', 'id'],
  });
export type SyncEvent = z.infer<typeof SyncEventSchema>;

export const SyncPushRequestSchema = z.object({
  protocol_version: z.literal(1),
  device_id: UuidSchema,
  events: z.array(SyncEventSchema).min(1).max(SYNC_PUSH_BATCH_LIMIT),
});
export type SyncPushRequest = z.infer<typeof SyncPushRequestSchema>;

export const SyncPushResponseSchema = z.object({
  /** Newly applied or already-seen (duplicate) — either way, mark as sent. */
  acknowledged: z.array(UuidSchema),
  rejected: z.array(
    z.object({
      event_id: UuidSchema,
      reason: z.string(),
      /** false = permanent (schema/permission) failure; park the event, don't retry. */
      retryable: z.boolean(),
    }),
  ),
  server_time: TimestampSchema,
});
export type SyncPushResponse = z.infer<typeof SyncPushResponseSchema>;

export const SyncPullRequestSchema = z.object({
  protocol_version: z.literal(1),
  device_id: UuidSchema,
  /** Opaque server cursor (monotonic change sequence); `null` = full bootstrap. */
  cursor: z.string().nullable(),
  limit: z.int().min(1).max(1000),
});
export type SyncPullRequest = z.infer<typeof SyncPullRequestSchema>;

export const SyncPullResponseSchema = z.object({
  changes: z.array(
    z.object({
      entity_type: SyncEntityTypeSchema,
      row: RowPayloadSchema,
      /**
       * Event that produced the server's current version of a LWW row (its
       * tie-breaker); `null` for append-only rows. The device compares
       * `(updated_at, event_id)` against its own pending edit exactly as the
       * server does, so both sides always pick the same winner.
       */
      event_id: UuidSchema.nullable(),
    }),
  ),
  next_cursor: z.string().nullable(),
  has_more: z.boolean(),
});
export type SyncPullResponse = z.infer<typeof SyncPullResponseSchema>;

/** Result of `sync_status` and payload of the `sync://status` event. */
export const SyncStatusSchema = z.object({
  /**
   * - `disabled`: no cloud configured for this client (offline-only install).
   * - `idle`: last attempt succeeded. `syncing`: a round is running.
   * - `offline`: the cloud could not be reached; changes are kept locally.
   * - `error`: the cloud refused the device (e.g. license needs re-activation).
   */
  state: z.enum(['disabled', 'idle', 'syncing', 'offline', 'error']),
  pending: NonNegativeIntSchema,
  /** Events the server permanently rejected (kept locally for diagnosis). */
  parked: NonNegativeIntSchema,
  last_synced_at: TimestampSchema.nullable(),
  last_error: z.string().nullable(),
});
export type SyncStatus = z.infer<typeof SyncStatusSchema>;

/** Result of the `sync_to_cloud` IPC command. */
export const SyncReportSchema = z.object({
  online: z.boolean(),
  pushed: NonNegativeIntSchema,
  rejected: NonNegativeIntSchema,
  pulled: NonNegativeIntSchema,
  pending: NonNegativeIntSchema,
  last_synced_at: TimestampSchema.nullable(),
});
export type SyncReport = z.infer<typeof SyncReportSchema>;
