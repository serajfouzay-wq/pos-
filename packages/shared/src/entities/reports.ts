import { z } from 'zod';
import { CurrencyCodeSchema } from '../currency';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import {
  EntityBaseSchema,
  NonNegativeIntSchema,
  PositiveIntSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';

/**
 * `z_reports` — APPEND-ONLY end-of-day closings, numbered per till. A Z
 * covers this till's transactions after the previous Z's `period_end` up to
 * its own. The headline figures are columns (for cloud queries); `report`
 * is the full snapshot that was printed, so a reprint is identical.
 * `grand_total` is the running net total of every Z of this till.
 */
export const ZReportSchema = EntityBaseSchema.extend({
  device_id: UuidSchema,
  z_number: PositiveIntSchema,
  period_start: TimestampSchema,
  period_end: TimestampSchema,
  run_by: UuidSchema,
  currency: CurrencyCodeSchema,
  sale_count: NonNegativeIntSchema,
  refund_count: NonNegativeIntSchema,
  void_count: NonNegativeIntSchema,
  gross_sales: NonNegativeMinorUnitsSchema,
  discount_total: NonNegativeMinorUnitsSchema,
  refund_total: NonNegativeMinorUnitsSchema,
  void_total: NonNegativeMinorUnitsSchema,
  net_sales: MinorUnitsSchema,
  tax_total: MinorUnitsSchema,
  grand_total: MinorUnitsSchema,
  report: z.json(),
});
export type ZReport = z.infer<typeof ZReportSchema>;
