import type { Permission, Session } from '@pos/shared';

/**
 * UI hint only — Rust re-checks every command. Uses the permission list Rust
 * sent with the session, so the UI and enforcement share one matrix.
 */
export function can(session: Session, permission: Permission): boolean {
  return session.permissions.includes(permission);
}
