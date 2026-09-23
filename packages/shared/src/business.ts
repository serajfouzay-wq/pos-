import { z } from 'zod';

/**
 * The client's business type reshapes the POS layout and workflow:
 * - retail: barcode entry, quick-keys grid, stock tracking, label printing
 * - cafe: menu grid with modifiers, table tabs, quick combos
 * - restaurant: table map, course sequencing, kitchen display, split bills
 */
export const BUSINESS_TYPES = ['retail', 'cafe', 'restaurant'] as const;
export const BusinessTypeSchema = z.enum(BUSINESS_TYPES);
export type BusinessType = z.infer<typeof BusinessTypeSchema>;
