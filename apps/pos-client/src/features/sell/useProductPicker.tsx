import { newUuid, type ComboWithItems, type Menu, type Product, type Uuid } from '@pos/shared';
import { useState, type ReactNode } from 'react';
import { ComboPicker, groupsFor, ModifierPicker } from './ModifierPicker';
import { newLine, type CartLine } from './lines';
import { WeightDialog } from './WeightDialog';

type Step =
  | { kind: 'weight'; product: Product }
  | { kind: 'options'; product: Product }
  | { kind: 'combo'; combo: ComboWithItems }
  | null;

interface Options {
  menu: Menu | undefined;
  products: ReadonlyMap<string, Product>;
  /** Course for new lines (restaurant), null elsewhere. */
  course: number | null;
  allowNote: boolean;
  onAdd: (lines: CartLine[]) => void;
}

/**
 * Turns a tap into cart lines: weighed goods ask the weight, products with
 * options ask them, combos ask every component's options.
 */
export function useProductPicker({ menu, products, course, allowNote, onAdd }: Options): {
  pick: (product: Product) => void;
  pickCombo: (combo: ComboWithItems) => void;
  dialogs: ReactNode;
  busy: boolean;
} {
  const [step, setStep] = useState<Step>(null);

  const pick = (product: Product) => {
    if (product.sold_by_weight) setStep({ kind: 'weight', product });
    // Only products with options ask; notes are added from the cart line.
    else if (groupsFor(menu, product.id).length > 0) setStep({ kind: 'options', product });
    else onAdd([newLine(product, { course })]);
  };

  const pickCombo = (combo: ComboWithItems) => {
    const needsChoices = combo.items.some((i) => groupsFor(menu, i.product_id).length > 0);
    if (needsChoices) setStep({ kind: 'combo', combo });
    else
      addCombo(
        combo,
        combo.items.map(() => []),
      );
  };

  const addCombo = (combo: ComboWithItems, choices: Uuid[][]) => {
    const ref = { combo_id: combo.id, instance: newUuid() };
    const lines = combo.items.flatMap((item, i) => {
      const product = products.get(item.product_id);
      return product
        ? [
            newLine(product, {
              quantity_milli: item.quantity_milli,
              modifier_ids: choices[i] ?? [],
              menu,
              course,
              combo: ref,
              combo_name: combo.name,
            }),
          ]
        : [];
    });
    if (lines.length === combo.items.length) onAdd(lines);
    setStep(null);
  };

  const close = () => {
    setStep(null);
  };

  const dialogs = (
    <>
      <WeightDialog
        key={step?.kind === 'weight' ? step.product.id : 'none'}
        product={step?.kind === 'weight' ? step.product : null}
        onClose={close}
        onConfirm={(milli) => {
          if (step?.kind === 'weight')
            onAdd([newLine(step.product, { quantity_milli: milli, course })]);
          close();
        }}
      />
      <ModifierPicker
        key={step?.kind === 'options' ? step.product.id : 'none-options'}
        product={step?.kind === 'options' ? step.product : null}
        menu={menu}
        allowNote={allowNote}
        onClose={close}
        onConfirm={({ modifier_ids, note }) => {
          if (step?.kind === 'options')
            onAdd([newLine(step.product, { modifier_ids, menu, course, note })]);
          close();
        }}
      />
      <ComboPicker
        key={step?.kind === 'combo' ? step.combo.id : 'none-combo'}
        combo={step?.kind === 'combo' ? step.combo : null}
        menu={menu}
        products={products}
        onClose={close}
        onConfirm={(choices) => {
          if (step?.kind === 'combo') addCombo(step.combo, choices);
        }}
      />
    </>
  );
  return { pick, pickCombo, dialogs, busy: step !== null };
}
