import { z } from 'zod';
import { UuidSchema } from '../primitives';

/**
 * The shop network: tills sync with one of them (the hub) over the local
 * network, with no internet. `off` = cloud (if the build has one) or none.
 */
export const LanRoleSchema = z.enum(['off', 'hub', 'client']);
export type LanRole = z.infer<typeof LanRoleSchema>;

export const LanSettingsSchema = z.object({
  role: LanRoleSchema,
  port: z.int().min(1024).max(65_534),
  /** The hub's `host:port` (tills joining a hub). */
  hub_address: z.string().max(100).nullable(),
  /** The pairing code shown on the hub (tills joining a hub). */
  hub_code: z.string().max(20).nullable(),
});
export type LanSettings = z.infer<typeof LanSettingsSchema>;

/** What the hub shows so tills can join. */
export const HubInfoSchema = z.object({
  code: z.string(),
  /** This PC's addresses on the shop network. */
  addresses: z.array(z.string()),
  port: z.int(),
  running: z.boolean(),
  /** Rows it holds, tills that have synced with it. */
  rows: z.int().nonnegative(),
  tills: z.int().nonnegative(),
});
export type HubInfo = z.infer<typeof HubInfoSchema>;

export const LanStatusSchema = z.object({
  settings: LanSettingsSchema,
  hub: HubInfoSchema.nullable(),
  last_error: z.string().nullable(),
});
export type LanStatus = z.infer<typeof LanStatusSchema>;

export const FoundHubSchema = z.object({
  address: z.string(),
  name: z.string(),
});
export type FoundHub = z.infer<typeof FoundHubSchema>;

export const HubHelloSchema = z.object({
  client_id: UuidSchema,
  hub_name: z.string(),
  rows: z.int().nonnegative(),
  tills: z.int().nonnegative(),
});
export type HubHello = z.infer<typeof HubHelloSchema>;
