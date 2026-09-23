/**
 * Events emitted by Rust to the frontend (`app.emit`). Names and payload
 * schemas are part of the IPC contract like commands are.
 */
import { LicenseStatusSchema } from '../license';

export const POS_EVENTS = {
  /** License status changed (activation, revocation, grace running out…). */
  license_status: { name: 'license://status', payload: LicenseStatusSchema },
} as const;
