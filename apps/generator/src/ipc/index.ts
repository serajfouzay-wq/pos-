/**
 * The generator UI's only gateway to Rust. GitHub, Supabase, license signing
 * keys and the filesystem are reachable exclusively through typed commands.
 */
import { createIpcClient, GENERATOR_IPC } from '@pos/shared';
import { invoke, isTauri } from '@tauri-apps/api/core';

export const ipc = createIpcClient(GENERATOR_IPC, (command, args) => invoke(command, args));

/** False when the UI is opened in a plain browser (e.g. `vite` without Tauri). */
export const inTauri = isTauri();
