/**
 * The frontend's only gateway to Rust. SQLite, printers, the cash drawer and
 * Supabase are reachable exclusively through these typed commands.
 */
import { createIpcClient, POS_IPC } from '@pos/shared';
import { invoke, isTauri } from '@tauri-apps/api/core';

export const ipc = createIpcClient(POS_IPC, (command, args) => invoke(command, args));

/** False when the UI is opened in a plain browser (e.g. `vite` without Tauri). */
export const inTauri = isTauri();
