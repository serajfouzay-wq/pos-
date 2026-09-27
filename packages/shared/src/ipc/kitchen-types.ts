/** The kitchen display (Phase 8): its window and the ticket board. */
import { z } from 'zod';
import {
  KitchenTicketKindSchema,
  KitchenTicketSchema,
  KitchenTicketStatusSchema,
} from '../entities/kitchen';
import { PositiveIntSchema, TimestampSchema, UuidSchema } from '../primitives';

export const KitchenDisplayStatusSchema = z.object({
  /** The build has the kitchen display (`features.kitchen_display`, not retail). */
  available: z.boolean(),
  /** This till shows the kitchen window (reopened at every start). */
  enabled: z.boolean(),
  /** The window is open now. */
  open: z.boolean(),
});
export type KitchenDisplayStatus = z.infer<typeof KitchenDisplayStatusSchema>;

export const KitchenBoardSchema = z.object({
  /** Open tickets, oldest first. */
  open: z.array(KitchenTicketSchema),
  /** Finished in the last `recent_minutes`, newest first (for recall). */
  ready: z.array(KitchenTicketSchema),
  /** For the ticket timers: the display's clock may differ from the till's. */
  server_time: TimestampSchema,
});
export type KitchenBoard = z.infer<typeof KitchenBoardSchema>;

/** `kitchen://changed` — a ticket was created, bumped, recalled or struck. */
export const KitchenChangeSchema = z.object({
  ticket_id: UuidSchema,
  ticket_number: PositiveIntSchema,
  title: z.string(),
  kind: KitchenTicketKindSchema,
  status: KitchenTicketStatusSchema,
});
export type KitchenChange = z.infer<typeof KitchenChangeSchema>;
