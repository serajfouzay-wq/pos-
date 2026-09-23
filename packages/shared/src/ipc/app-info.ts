import { z } from 'zod';
import { ClientConfigSchema } from '../client-config';

const AppInfoBaseSchema = z.object({
  version: z.string().min(1),
  build_profile: z.enum(['debug', 'release']),
  /** Rust target triple the binary was compiled for, e.g. `x86_64-pc-windows-msvc`. */
  target: z.string().min(1),
});

export const PosAppInfoSchema = AppInfoBaseSchema.extend({
  app: z.literal('pos-client'),
  client: ClientConfigSchema,
});
export type PosAppInfo = z.infer<typeof PosAppInfoSchema>;

export const GeneratorAppInfoSchema = AppInfoBaseSchema.extend({
  app: z.literal('generator'),
});
export type GeneratorAppInfo = z.infer<typeof GeneratorAppInfoSchema>;
