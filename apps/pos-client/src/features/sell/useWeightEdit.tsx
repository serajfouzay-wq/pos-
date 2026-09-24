import type { Uuid } from '@pos/shared';
import { useState, type ReactNode } from 'react';
import type { CartLine } from './lines';
import { WeightDialog } from './WeightDialog';

/** Re-weighing a weighed line already in the cart or order. */
export function useWeightEdit(onSet: (lineId: Uuid, quantityMilli: number) => void): {
  edit: (line: CartLine) => void;
  dialog: ReactNode;
  busy: boolean;
} {
  const [line, setLine] = useState<CartLine | null>(null);
  const dialog = (
    <WeightDialog
      key={line?.line_id ?? 'none'}
      product={line}
      initialMilli={line?.quantity_milli ?? 0}
      onClose={() => {
        setLine(null);
      }}
      onConfirm={(milli) => {
        if (line) onSet(line.line_id, milli);
        setLine(null);
      }}
    />
  );
  return { edit: setLine, dialog, busy: line !== null };
}
