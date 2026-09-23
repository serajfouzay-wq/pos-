import { useEffect, useLayoutEffect, useRef } from 'react';
import { ScanDetector } from './scanner';

function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.closest('[data-scanner="allow"]')) return false;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
}

/**
 * Global scanner hook. Listens in the CAPTURE phase so a scan is seen before
 * any focused button reacts to its Enter. While focus is in a text field the
 * keys belong to the field (unless it opts in with `data-scanner="allow"`).
 */
export function useBarcodeScanner(onScan: (code: string) => void, enabled = true): void {
  const handler = useRef(onScan);
  useLayoutEffect(() => {
    handler.current = onScan;
  });

  useEffect(() => {
    if (!enabled) return undefined;
    const detector = new ScanDetector();
    const listener = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.altKey || event.metaKey || isEditable(event.target)) return;
      const result = detector.feed(event.key, event.timeStamp);
      if (result.kind === 'scan') {
        event.preventDefault();
        event.stopPropagation();
        handler.current(result.code);
      } else if (result.kind === 'buffering' && detector.inBurst) {
        event.preventDefault();
      }
    };
    window.addEventListener('keydown', listener, { capture: true });
    return () => {
      window.removeEventListener('keydown', listener, { capture: true });
    };
  }, [enabled]);
}
