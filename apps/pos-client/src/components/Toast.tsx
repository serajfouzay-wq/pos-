import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState, type ReactNode } from 'react';

/** A short status message at the bottom of the screen. */
export function useToast(timeoutMs = 2500): { show: (message: string) => void; node: ReactNode } {
  const [message, setMessage] = useState<string | null>(null);
  useEffect(() => {
    if (!message) return undefined;
    const id = setTimeout(() => {
      setMessage(null);
    }, timeoutMs);
    return () => {
      clearTimeout(id);
    };
  }, [message, timeoutMs]);

  const node = (
    <AnimatePresence>
      {message && (
        <motion.div
          className="toast"
          role="status"
          initial={{ y: 40, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ opacity: 0 }}
        >
          {message}
        </motion.div>
      )}
    </AnimatePresence>
  );
  return { show: setMessage, node };
}

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
