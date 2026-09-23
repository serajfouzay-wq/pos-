/**
 * Per-client build configuration.
 *
 * The generator app produces one of these per client; the build pipeline
 * validates it and embeds it into the compiled POS binary (see
 * `apps/pos-client/src-tauri/build.rs`). The POS frontend reads it only via the
 * `app_info` IPC command, never from disk.
 *
 * Mirrored by `pos_core::config::ClientConfig`.
 */
import { z } from 'zod';
import { BusinessTypeSchema } from './business';
import { CurrencyCodeSchema } from './currency';
import { LocaleSchema } from './i18n';
import { BasisPointsSchema, HexColorSchema, UuidSchema } from './primitives';

export const CLIENT_CONFIG_SCHEMA_VERSION = 1;

export const ReceiptLayoutSchema = z.object({
  /** Asset path relative to the client's asset bundle, e.g. `receipt-logo.png`. */
  logo_asset: z.string().min(1).nullable(),
  header_lines: z.array(z.string().max(48)).max(6),
  footer_text: z.string().max(240),
  show_tax_number: z.boolean(),
  paper_width_mm: z.union([z.literal(58), z.literal(80)]),
});
export type ReceiptLayout = z.infer<typeof ReceiptLayoutSchema>;

export const ClientConfigSchema = z
  .object({
    schema_version: z.literal(CLIENT_CONFIG_SCHEMA_VERSION),
    client_id: UuidSchema,
    /** URL/file-safe identifier used for installer names and bundle ids. */
    client_slug: z
      .string()
      .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/, 'kebab-case only')
      .max(40),
    display_name: z.string().min(1).max(80),
    business_type: BusinessTypeSchema,
    locale: z.object({
      default: LocaleSchema,
      supported: z.array(LocaleSchema).min(1),
    }),
    currency: z.object({
      base: CurrencyCodeSchema,
      /** Additional currencies accepted at the till / shown as conversions. */
      accepted: z.array(CurrencyCodeSchema),
    }),
    tax: z.object({
      registration_number: z.string().max(40).nullable(),
      prices_include_tax: z.boolean(),
      default_rate_bps: BasisPointsSchema,
    }),
    receipt: ReceiptLayoutSchema,
    branding: z.object({
      primary_color: HexColorSchema,
      accent_color: HexColorSchema,
    }),
    features: z.object({
      loyalty: z.boolean(),
      kitchen_display: z.boolean(),
      multi_currency: z.boolean(),
      purchase_orders: z.boolean(),
    }),
  })
  .refine((config) => config.locale.supported.includes(config.locale.default), {
    message: 'locale.default must be one of locale.supported',
    path: ['locale', 'default'],
  })
  .refine((config) => !config.currency.accepted.includes(config.currency.base), {
    message: 'currency.accepted must not repeat the base currency',
    path: ['currency', 'accepted'],
  });
export type ClientConfig = z.infer<typeof ClientConfigSchema>;
