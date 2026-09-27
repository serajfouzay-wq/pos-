/**
 * Events emitted by Rust to the frontend (`app.emit`). Names and payload
 * schemas are part of the IPC contract like commands are.
 */
import { LicenseStatusSchema } from '../license';
import { SyncStatusSchema } from '../sync';
import { KitchenChangeSchema } from './kitchen-types';
import { PrinterStatusSchema } from './pos-types';
import { UpdateStatusSchema } from './updater-types';

export const POS_EVENTS = {
  /** License status changed (activation, revocation, grace running out…). */
  license_status: { name: 'license://status', payload: LicenseStatusSchema },
  /** Printer reachability / offline queue length changed. */
  printer_status: { name: 'printer://status', payload: PrinterStatusSchema },
  /** Sync state / pending count changed. */
  sync_status: { name: 'sync://status', payload: SyncStatusSchema },
  /** A kitchen ticket was created or changed (on this till or by sync). */
  kitchen_changed: { name: 'kitchen://changed', payload: KitchenChangeSchema },
  /** Update check / download progress / ready to install. */
  update_status: { name: 'updater://status', payload: UpdateStatusSchema },
} as const;
