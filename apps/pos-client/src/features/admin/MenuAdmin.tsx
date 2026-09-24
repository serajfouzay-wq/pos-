import {
  parseDecimalString,
  toDecimalString,
  type ComboInput,
  type ComboWithItems,
  type ModifierGroupInput,
  type ModifierGroupWithOptions,
  type Uuid,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { errorText } from '../../components/Toast';
import {
  useDeleteCombo,
  useDeleteModifierGroup,
  useMenu,
  useProducts,
  useSaveCombo,
  useSaveModifierGroup,
} from '../../ipc/queries';
import { useMoney } from '../../lib/money';

interface OptionDraft {
  id: Uuid | null;
  name: string;
  price_delta: string;
  is_default: boolean;
  is_active: boolean;
}

interface GroupDraft {
  id: Uuid | null;
  name: string;
  min_select: number;
  max_select: number;
  sort_order: number;
  is_active: boolean;
  modifiers: OptionDraft[];
}

interface ComboDraft {
  id: Uuid | null;
  name: string;
  price: string;
  color: string | null;
  sort_order: number;
  is_active: boolean;
  items: { product_id: Uuid | ''; units: number }[];
}

/** Two taps to delete: the first arms the button. */
function DeleteButton({ onConfirm, disabled }: { onConfirm: () => void; disabled: boolean }) {
  const { t } = useTranslation();
  const [armed, setArmed] = useState(false);
  return (
    <button
      type="button"
      className={armed ? 'button button--danger' : 'link-button'}
      disabled={disabled}
      onBlur={() => {
        setArmed(false);
      }}
      onClick={() => {
        if (armed) onConfirm();
        setArmed(!armed);
      }}
    >
      {armed ? t('admin.menu.confirmDelete') : t('admin.menu.delete')}
    </button>
  );
}

function GroupEditor({
  draft,
  onChange,
  onDone,
}: {
  draft: GroupDraft;
  onChange: (draft: GroupDraft) => void;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const { currency } = useMoney();
  const save = useSaveModifierGroup();
  const [error, setError] = useState<string | null>(null);
  const setOption = (i: number, patch: Partial<OptionDraft>) => {
    onChange({
      ...draft,
      modifiers: draft.modifiers.map((m, j) => (j === i ? { ...m, ...patch } : m)),
    });
  };

  const submit = () => {
    let input: ModifierGroupInput;
    try {
      input = {
        ...draft,
        modifiers: draft.modifiers.map((m) => ({
          ...m,
          price_delta: parseDecimalString(m.price_delta || '0', currency),
        })),
      };
    } catch (e) {
      setError(errorText(e));
      return;
    }
    if (input.min_select > input.max_select) {
      setError(t('admin.menu.badRange'));
      return;
    }
    setError(null);
    save.mutate(input, { onSuccess: onDone });
  };

  return (
    <form
      className="stack"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="form-grid">
        <label className="field">
          {t('admin.menu.groupName')}
          <input
            required
            maxLength={80}
            value={draft.name}
            onChange={(e) => {
              onChange({ ...draft, name: e.target.value });
            }}
          />
        </label>
        <label className="field">
          {t('admin.menu.minSelect')}
          <input
            type="number"
            min={0}
            max={20}
            value={draft.min_select}
            onChange={(e) => {
              onChange({ ...draft, min_select: Math.max(0, Math.trunc(Number(e.target.value))) });
            }}
          />
        </label>
        <label className="field">
          {t('admin.menu.maxSelect')}
          <input
            type="number"
            min={1}
            max={20}
            value={draft.max_select}
            onChange={(e) => {
              onChange({ ...draft, max_select: Math.max(1, Math.trunc(Number(e.target.value))) });
            }}
          />
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.is_active}
            onChange={(e) => {
              onChange({ ...draft, is_active: e.target.checked });
            }}
          />
          {t('admin.menu.active')}
        </label>
      </div>
      <table className="table table--compact">
        <thead>
          <tr>
            <th>{t('admin.menu.optionName')}</th>
            <th>{t('admin.menu.priceDelta', { currency })}</th>
            <th>{t('admin.menu.isDefault')}</th>
            <th>{t('admin.menu.active')}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {draft.modifiers.map((m, i) => (
            <tr key={m.id ?? `new-${String(i)}`}>
              <td>
                <input
                  required
                  maxLength={80}
                  value={m.name}
                  onChange={(e) => {
                    setOption(i, { name: e.target.value });
                  }}
                />
              </td>
              <td>
                <input
                  dir="ltr"
                  inputMode="decimal"
                  placeholder="0"
                  value={m.price_delta}
                  onChange={(e) => {
                    setOption(i, { price_delta: e.target.value });
                  }}
                />
              </td>
              <td>
                <input
                  type="checkbox"
                  checked={m.is_default}
                  onChange={(e) => {
                    setOption(i, { is_default: e.target.checked });
                  }}
                />
              </td>
              <td>
                <input
                  type="checkbox"
                  checked={m.is_active}
                  onChange={(e) => {
                    setOption(i, { is_active: e.target.checked });
                  }}
                />
              </td>
              <td>
                {m.id === null && (
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t('sell.remove')}
                    onClick={() => {
                      onChange({ ...draft, modifiers: draft.modifiers.filter((_, j) => j !== i) });
                    }}
                  >
                    ✕
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <button
        type="button"
        className="link-button"
        onClick={() => {
          onChange({
            ...draft,
            modifiers: [
              ...draft.modifiers,
              { id: null, name: '', price_delta: '', is_default: false, is_active: true },
            ],
          });
        }}
      >
        + {t('admin.menu.addOption')}
      </button>
      {(error ?? save.error?.message) && (
        <p role="alert" className="error-text">
          {error ?? save.error?.message}
        </p>
      )}
      <div className="row">
        <button
          type="submit"
          className="button button--primary"
          disabled={save.isPending || draft.modifiers.length === 0}
        >
          {t('common.save')}
        </button>
        <button type="button" className="button" onClick={onDone}>
          {t('common.cancel')}
        </button>
      </div>
    </form>
  );
}

function ComboEditor({
  draft,
  onChange,
  onDone,
}: {
  draft: ComboDraft;
  onChange: (draft: ComboDraft) => void;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const { currency, format } = useMoney();
  const products = useProducts({ limit: 1000 });
  const save = useSaveCombo();
  const [error, setError] = useState<string | null>(null);
  const priceOf = (id: Uuid | '') => products.data?.find((p) => p.id === id)?.price ?? 0;
  const separately = draft.items.reduce((sum, i) => sum + priceOf(i.product_id) * i.units, 0);

  const submit = () => {
    const items = draft.items.flatMap((i) =>
      i.product_id ? [{ product_id: i.product_id, quantity_milli: i.units * 1000 }] : [],
    );
    let input: ComboInput;
    try {
      input = { ...draft, price: parseDecimalString(draft.price || '0', currency), items };
    } catch (e) {
      setError(errorText(e));
      return;
    }
    if (items.length < 2) {
      setError(t('admin.menu.comboTooSmall'));
      return;
    }
    setError(null);
    save.mutate(input, { onSuccess: onDone });
  };

  return (
    <form
      className="stack"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="form-grid">
        <label className="field">
          {t('admin.menu.comboName')}
          <input
            required
            maxLength={80}
            value={draft.name}
            onChange={(e) => {
              onChange({ ...draft, name: e.target.value });
            }}
          />
        </label>
        <label className="field">
          {t('admin.menu.comboPrice', { currency })}
          <input
            dir="ltr"
            inputMode="decimal"
            value={draft.price}
            onChange={(e) => {
              onChange({ ...draft, price: e.target.value });
            }}
          />
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.is_active}
            onChange={(e) => {
              onChange({ ...draft, is_active: e.target.checked });
            }}
          />
          {t('admin.menu.active')}
        </label>
      </div>
      <ul className="stack">
        {draft.items.map((item, i) => (
          <li key={i} className="row">
            <select
              className="grow"
              value={item.product_id}
              onChange={(e) => {
                const id = products.data?.find((p) => p.id === e.target.value)?.id ?? '';
                onChange({
                  ...draft,
                  items: draft.items.map((x, j) => (j === i ? { ...x, product_id: id } : x)),
                });
              }}
            >
              <option value="">—</option>
              {products.data
                ?.filter((p) => !p.sold_by_weight)
                .map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} · {format(p.price)}
                  </option>
                ))}
            </select>
            <div className="stepper" dir="ltr">
              <button
                type="button"
                disabled={item.units <= 1}
                aria-label={t('sell.less')}
                onClick={() => {
                  onChange({
                    ...draft,
                    items: draft.items.map((x, j) => (j === i ? { ...x, units: x.units - 1 } : x)),
                  });
                }}
              >
                −
              </button>
              <span>{item.units}</span>
              <button
                type="button"
                disabled={item.units >= 9}
                aria-label={t('sell.more')}
                onClick={() => {
                  onChange({
                    ...draft,
                    items: draft.items.map((x, j) => (j === i ? { ...x, units: x.units + 1 } : x)),
                  });
                }}
              >
                +
              </button>
            </div>
            <button
              type="button"
              className="icon-button"
              aria-label={t('sell.remove')}
              onClick={() => {
                onChange({ ...draft, items: draft.items.filter((_, j) => j !== i) });
              }}
            >
              ✕
            </button>
          </li>
        ))}
      </ul>
      <button
        type="button"
        className="link-button"
        disabled={draft.items.length >= 12}
        onClick={() => {
          onChange({ ...draft, items: [...draft.items, { product_id: '', units: 1 }] });
        }}
      >
        + {t('admin.menu.addItem')}
      </button>
      <p className="muted small">{t('admin.menu.separately', { amount: format(separately) })}</p>
      {(error ?? save.error?.message) && (
        <p role="alert" className="error-text">
          {error ?? save.error?.message}
        </p>
      )}
      <div className="row">
        <button type="submit" className="button button--primary" disabled={save.isPending}>
          {t('common.save')}
        </button>
        <button type="button" className="button" onClick={onDone}>
          {t('common.cancel')}
        </button>
      </div>
    </form>
  );
}

/** Back office: option groups (size, milk…) and combos. */
export function MenuAdmin() {
  const { t } = useTranslation();
  const { currency, format } = useMoney();
  const menu = useMenu(true);
  const products = useProducts({ include_inactive: true, limit: 1000 });
  const deleteGroup = useDeleteModifierGroup();
  const deleteCombo = useDeleteCombo();
  const [group, setGroup] = useState<GroupDraft | null>(null);
  const [combo, setCombo] = useState<ComboDraft | null>(null);
  const plain = (amount: number) => toDecimalString(amount, currency);
  const nameOf = (id: Uuid) => products.data?.find((p) => p.id === id)?.name ?? '…';
  const groups = menu.data?.modifier_groups ?? [];
  const combos = menu.data?.combos ?? [];
  const usage = (groupId: Uuid) =>
    Object.values(menu.data?.product_modifier_groups ?? {}).filter((ids) => ids.includes(groupId))
      .length;

  const editGroup = (g: ModifierGroupWithOptions | null) => {
    setGroup({
      id: g?.id ?? null,
      name: g?.name ?? '',
      min_select: g?.min_select ?? 0,
      max_select: g?.max_select ?? 1,
      sort_order: g?.sort_order ?? groups.length,
      is_active: g?.is_active ?? true,
      modifiers: g
        ? g.modifiers.map((m) => ({
            id: m.id,
            name: m.name,
            price_delta: m.price_delta === 0 ? '' : plain(m.price_delta),
            is_default: m.is_default,
            is_active: m.is_active,
          }))
        : [{ id: null, name: '', price_delta: '', is_default: true, is_active: true }],
    });
  };

  const editCombo = (c: ComboWithItems | null) => {
    setCombo({
      id: c?.id ?? null,
      name: c?.name ?? '',
      price: c ? plain(c.price) : '',
      color: c?.color ?? null,
      sort_order: c?.sort_order ?? combos.length,
      is_active: c?.is_active ?? true,
      items: c
        ? c.items.map((i) => ({
            product_id: i.product_id,
            units: Math.max(1, i.quantity_milli / 1000),
          }))
        : [
            { product_id: '', units: 1 },
            { product_id: '', units: 1 },
          ],
    });
  };

  const deleteError = deleteGroup.error ?? deleteCombo.error;

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('admin.menu.title')}</h1>
      </header>
      {deleteError && (
        <p role="alert" className="error-text">
          {deleteError.message}
        </p>
      )}

      <section className="stack">
        <header className="admin__header">
          <h2>{t('admin.menu.groups')}</h2>
          <button
            type="button"
            className="button button--primary"
            onClick={() => {
              editGroup(null);
            }}
          >
            {t('admin.menu.addGroup')}
          </button>
        </header>
        <p className="muted">{t('admin.menu.groupsHelp')}</p>
        <Modal
          open={group !== null}
          wide
          title={group?.id ? group.name : t('admin.menu.addGroup')}
          onClose={() => {
            setGroup(null);
          }}
        >
          {group && (
            <GroupEditor
              draft={group}
              onChange={setGroup}
              onDone={() => {
                setGroup(null);
              }}
            />
          )}
        </Modal>
        <div className="card-list">
          {groups.length === 0 && <p className="muted">{t('admin.menu.noGroups')}</p>}
          {groups.map((g) => (
            <article key={g.id} className={g.is_active ? 'card' : 'card inactive'}>
              <header className="row">
                <strong className="grow">{g.name}</strong>
                <span className="muted small">
                  {t('admin.menu.rule', { min: g.min_select, max: g.max_select })}
                </span>
              </header>
              <p className="small">
                {g.modifiers
                  .map((m) => (m.price_delta ? `${m.name} (+${format(m.price_delta)})` : m.name))
                  .join(' · ')}
              </p>
              <footer className="row">
                <span className="muted small grow">
                  {t('admin.menu.usedBy', { count: usage(g.id) })}
                </span>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    editGroup(g);
                  }}
                >
                  {t('common.edit')}
                </button>
                <DeleteButton
                  disabled={deleteGroup.isPending}
                  onConfirm={() => {
                    deleteGroup.mutate(g.id);
                  }}
                />
              </footer>
            </article>
          ))}
        </div>
      </section>

      <section className="stack">
        <header className="admin__header">
          <h2>{t('admin.menu.combos')}</h2>
          <button
            type="button"
            className="button button--primary"
            onClick={() => {
              editCombo(null);
            }}
          >
            {t('admin.menu.addCombo')}
          </button>
        </header>
        <p className="muted">{t('admin.menu.combosHelp')}</p>
        <Modal
          open={combo !== null}
          wide
          title={combo?.id ? combo.name : t('admin.menu.addCombo')}
          onClose={() => {
            setCombo(null);
          }}
        >
          {combo && (
            <ComboEditor
              draft={combo}
              onChange={setCombo}
              onDone={() => {
                setCombo(null);
              }}
            />
          )}
        </Modal>
        <div className="card-list">
          {combos.length === 0 && <p className="muted">{t('admin.menu.noCombos')}</p>}
          {combos.map((c) => (
            <article key={c.id} className={c.is_active ? 'card' : 'card inactive'}>
              <header className="row">
                <strong className="grow">{c.name}</strong>
                <span>{format(c.price)}</span>
              </header>
              <p className="small">{c.items.map((i) => nameOf(i.product_id)).join(' + ')}</p>
              <footer className="row row--end">
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    editCombo(c);
                  }}
                >
                  {t('common.edit')}
                </button>
                <DeleteButton
                  disabled={deleteCombo.isPending}
                  onConfirm={() => {
                    deleteCombo.mutate(c.id);
                  }}
                />
              </footer>
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}
