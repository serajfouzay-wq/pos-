import type { ComboWithItems, Menu, ModifierGroupWithOptions, Product, Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { useMoney } from '../../lib/money';

/** The groups a product asks, in order (only live ones). */
export function groupsFor(menu: Menu | undefined, productId: string): ModifierGroupWithOptions[] {
  const links = menu?.product_modifier_groups as
    Readonly<Record<string, readonly string[]>> | undefined;
  const ids = links?.[productId] ?? [];
  return ids
    .map((id) => menu?.modifier_groups.find((g) => g.id === id))
    .filter((g): g is ModifierGroupWithOptions => g !== undefined && g.modifiers.length > 0);
}

function defaults(groups: readonly ModifierGroupWithOptions[]): Uuid[] {
  return groups.flatMap((g) =>
    g.modifiers
      .filter((m) => m.is_default && m.is_active)
      .slice(0, g.max_select)
      .map((m) => m.id),
  );
}

function valid(groups: readonly ModifierGroupWithOptions[], chosen: readonly Uuid[]) {
  return groups.every((g) => {
    const n = g.modifiers.filter((m) => chosen.includes(m.id)).length;
    return n >= g.min_select && n <= g.max_select;
  });
}

/** One product's option groups (radio for single choice, toggles otherwise). */
function OptionGroups({
  groups,
  chosen,
  onChange,
}: {
  groups: readonly ModifierGroupWithOptions[];
  chosen: readonly Uuid[];
  onChange: (ids: Uuid[]) => void;
}) {
  const { t } = useTranslation();
  const { format } = useMoney();
  return (
    <>
      {groups.map((group) => {
        const picked = group.modifiers.filter((m) => chosen.includes(m.id));
        const single = group.max_select === 1;
        return (
          <fieldset key={group.id} className="option-group">
            <legend>
              {group.name}{' '}
              <span className="muted small">
                {group.min_select > 0
                  ? t('options.required', { count: group.min_select })
                  : t('options.upTo', { count: group.max_select })}
              </span>
            </legend>
            <div className="option-group__choices">
              {group.modifiers.map((m) => {
                const on = chosen.includes(m.id);
                return (
                  <button
                    key={m.id}
                    type="button"
                    className="option"
                    aria-pressed={on}
                    disabled={!on && !single && picked.length >= group.max_select}
                    onClick={() => {
                      const others = chosen.filter(
                        (id) => !group.modifiers.some((x) => x.id === id),
                      );
                      if (single)
                        onChange(on && group.min_select === 0 ? others : [...others, m.id]);
                      else onChange(on ? chosen.filter((id) => id !== m.id) : [...chosen, m.id]);
                    }}
                  >
                    <span>{m.name}</span>
                    {m.price_delta !== 0 && (
                      <span className="muted small">
                        {m.price_delta > 0 ? '+' : '−'}
                        {format(Math.abs(m.price_delta))}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </fieldset>
        );
      })}
    </>
  );
}

interface PickerProps {
  product: Product | null;
  menu: Menu | undefined;
  allowNote: boolean;
  onConfirm: (choice: { modifier_ids: Uuid[]; note: string | null }) => void;
  onClose: () => void;
}

/** Options (and a kitchen note) for one product. */
export function ModifierPicker({ product, menu, allowNote, onConfirm, onClose }: PickerProps) {
  const { t } = useTranslation();
  const groups = product ? groupsFor(menu, product.id) : [];
  const [chosen, setChosen] = useState<Uuid[]>(() => defaults(groups));
  const [note, setNote] = useState('');
  const ok = valid(groups, chosen);
  return (
    <Modal open={product !== null} title={product?.name ?? ''} onClose={onClose} wide>
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          if (ok) onConfirm({ modifier_ids: chosen, note: note.trim() || null });
        }}
      >
        <OptionGroups groups={groups} chosen={chosen} onChange={setChosen} />
        {allowNote && (
          <label className="field">
            <span>{t('options.note')}</span>
            <input
              value={note}
              maxLength={200}
              placeholder={t('options.notePlaceholder')}
              onChange={(e) => {
                setNote(e.target.value);
              }}
            />
          </label>
        )}
        <button
          type="submit"
          className="button button--primary button--block button--xl"
          disabled={!ok}
        >
          {t('options.add')}
        </button>
      </form>
    </Modal>
  );
}

interface ComboProps {
  combo: ComboWithItems | null;
  menu: Menu | undefined;
  products: ReadonlyMap<string, Product>;
  onConfirm: (choices: Uuid[][]) => void;
  onClose: () => void;
}

/** Options for every component of a combo that asks some, on one screen. */
export function ComboPicker({ combo, menu, products, onConfirm, onClose }: ComboProps) {
  const { t } = useTranslation();
  const parts = (combo?.items ?? []).map((item) => ({
    item,
    product: products.get(item.product_id),
    groups: groupsFor(menu, item.product_id),
  }));
  const [chosen, setChosen] = useState<Uuid[][]>(() => parts.map((p) => defaults(p.groups)));
  const ok = parts.every((p, i) => p.product !== undefined && valid(p.groups, chosen[i] ?? []));
  return (
    <Modal open={combo !== null} title={combo?.name ?? ''} onClose={onClose} wide>
      <div className="stack">
        {parts.map((part, i) => (
          <section key={part.item.id} className="combo-part">
            <h3>{part.product?.name ?? t('options.unavailable')}</h3>
            {part.groups.length === 0 ? (
              <p className="muted small">{t('options.noChoices')}</p>
            ) : (
              <OptionGroups
                groups={part.groups}
                chosen={chosen[i] ?? []}
                onChange={(ids) => {
                  setChosen((all) => all.map((c, j) => (j === i ? ids : c)));
                }}
              />
            )}
          </section>
        ))}
        <button
          type="button"
          className="button button--primary button--block button--xl"
          disabled={!ok}
          onClick={() => {
            onConfirm(chosen);
          }}
        >
          {t('options.addCombo')}
        </button>
      </div>
    </Modal>
  );
}
