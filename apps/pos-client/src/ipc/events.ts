/**
 * Typed subscriptions to events emitted by Rust. Payloads are validated like
 * command results; a malformed payload is dropped rather than trusted.
 */
import { POS_EVENTS } from '@pos/shared';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { z } from 'zod';
import { inTauri } from './index';

type EventKey = keyof typeof POS_EVENTS;
type Payload<K extends EventKey> = z.output<(typeof POS_EVENTS)[K]['payload']>;

export function subscribe<K extends EventKey>(
  key: K,
  handler: (payload: Payload<K>) => void,
): () => void {
  if (!inTauri) return () => undefined;
  const { name, payload } = POS_EVENTS[key];
  let unlisten: UnlistenFn | undefined;
  let disposed = false;
  void listen<unknown>(name, (event) => {
    const parsed = payload.safeParse(event.payload);
    if (parsed.success) handler(parsed.data as Payload<K>);
  }).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });
  return () => {
    disposed = true;
    unlisten?.();
  };
}
