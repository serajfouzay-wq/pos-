/**
 * Kitchen tickets: what the kitchen display (KDS) shows and the kitchen
 * printer prints. A ticket is written when a course is sent, when items
 * already sent are changed or removed (a `void` ticket), and — when the
 * build has the kitchen display — for pay-now sales of a cafe or
 * restaurant. Synced last-write-wins, so a kitchen screen on another PC
 * sees them and every till sees what the kitchen has finished.
 */
import { z } from 'zod';
import {
  EntityBaseSchema,
  NonNegativeIntSchema,
  PositiveIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';
import { OrderTypeSchema } from './sales';

export const KITCHEN_TICKET_KINDS = ['order', 'void'] as const;
export const KitchenTicketKindSchema = z.enum(KITCHEN_TICKET_KINDS);
export type KitchenTicketKind = z.infer<typeof KitchenTicketKindSchema>;

export const KITCHEN_TICKET_STATUSES = ['open', 'ready'] as const;
export const KitchenTicketStatusSchema = z.enum(KITCHEN_TICKET_STATUSES);
export type KitchenTicketStatus = z.infer<typeof KitchenTicketStatusSchema>;

export const KitchenTicketItemSchema = z.object({
  /** The open-order line, or a fresh id for a pay-now sale's line. */
  line_id: UuidSchema,
  quantity_milli: QuantityMilliSchema,
  name: z.string().min(1).max(120),
  modifiers: z.array(z.string().max(80)).max(20),
  note: z.string().max(200).nullable(),
  course: PositiveIntSchema.max(9).nullable(),
  /** Struck through on the display by the cook. */
  done_at: TimestampSchema.nullable(),
});
export type KitchenTicketItem = z.infer<typeof KitchenTicketItemSchema>;

export const KitchenTicketSchema = EntityBaseSchema.extend({
  device_id: UuidSchema,
  /** Per till, from 1 — "#42" on the display. */
  ticket_number: PositiveIntSchema,
  kind: KitchenTicketKindSchema,
  order_id: UuidSchema.nullable(),
  transaction_id: UuidSchema.nullable(),
  /** "Table T4", "Tab Sara", "Takeaway D01-000123". */
  title: z.string().min(1).max(80),
  order_type: OrderTypeSchema,
  /** The course sent; null = everything / not coursed. */
  course: PositiveIntSchema.max(9).nullable(),
  server_name: z.string().max(80),
  guests: NonNegativeIntSchema.max(99),
  items: z.array(KitchenTicketItemSchema).min(1).max(200),
  status: KitchenTicketStatusSchema,
  fired_at: TimestampSchema,
  ready_at: TimestampSchema.nullable(),
});
export type KitchenTicket = z.infer<typeof KitchenTicketSchema>;
