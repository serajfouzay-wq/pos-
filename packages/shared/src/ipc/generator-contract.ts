/**
 * IPC contract of the generator app (`apps/generator/src-tauri`).
 * Client management, asset upload and build triggering arrive in Phase 5.
 */
import { z } from 'zod';
import { GeneratorAppInfoSchema } from './app-info';
import { command } from './contract';

export const GENERATOR_IPC = {
  app_info: command(z.object({}), GeneratorAppInfoSchema, 1),
} as const;

export type GeneratorIpcContract = typeof GENERATOR_IPC;
