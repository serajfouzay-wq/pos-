import { z } from 'zod';

/** RFC 4122 UUID. Every row id and every sync event id is a UUID. */
export const UuidSchema = z.uuid().brand<'Uuid'>();
export type Uuid = z.infer<typeof UuidSchema>;

/**
 * UTC ISO-8601 timestamp with millisecond precision, e.g. `2026-09-23T10:15:30.123Z`.
 *
 * The fixed format is load-bearing: last-write-wins conflict resolution compares
 * `updated_at` values, and a fixed-width UTC format makes lexical order equal
 * chronological order in both SQLite and PostgreSQL.
 */
export const TimestampSchema = z.iso.datetime({ precision: 3 }).brand<'Timestamp'>();
export type Timestamp = z.infer<typeof TimestampSchema>;

/** Safe integer (|n| <= 2^53 - 1). */
export const IntSchema = z.int();
export const NonNegativeIntSchema = z.int().nonnegative();
export const PositiveIntSchema = z.int().positive();

/**
 * Quantity in thousandths of a unit (1000 = 1 unit, 250 = 0.25 kg).
 * Keeps weighed/fractional items in integer arithmetic.
 */
export const QuantityMilliSchema = z.int().positive();
export type QuantityMilli = z.infer<typeof QuantityMilliSchema>;
export const QUANTITY_SCALE = 1000;

/** Percentage expressed in basis points: 10_000 bps = 100 %, 500 bps = 5 %. */
export const BasisPointsSchema = z.int().min(0).max(10_000);
export type BasisPoints = z.infer<typeof BasisPointsSchema>;
export const BPS_SCALE = 10_000;

/**
 * Columns present on every table. Rows are never hard-deleted; `deleted_at`
 * is set instead, and the tombstone syncs like any other update.
 */
export const EntityBaseSchema = z.object({
  id: UuidSchema,
  created_at: TimestampSchema,
  updated_at: TimestampSchema,
  deleted_at: TimestampSchema.nullable(),
});
export type EntityBase = z.infer<typeof EntityBaseSchema>;

export const HexColorSchema = z.string().regex(/^#[0-9a-fA-F]{6}$/, 'Expected #RRGGBB');
