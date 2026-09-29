import {
  parseDecimalString,
  toDecimalString,
  type DiscountApplyMode,
  type DiscountRule,
  type DiscountRuleInput,
  type DiscountRuleView,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import {
  useCategories,
  useDeleteDiscountRule,
  useDiscountRules,
  useProducts,
  useSaveDiscountRule,
} from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { useUiStore } from '../../stores/ui';
import {
  dateToTimestamp,
  dayNames,
  daysSummary,
  EVERY_DAY,
  formatPercent,
  minutesToTime,
  parsePercent,
  timestampToDate,
  timeToMinutes,
  WEEKDAYS,
  WEEKEND,
} from './schedule';

type Kind = DiscountRule['kind'];
type Scope = DiscountRule['scope'];

/** What the form edits: text fields stay text until saved. */
interface Draft {
  id: DiscountRule['id'] | null;
  name: string;
  kind: Kind;
  value: string;
  scope: Scope;
  target_id: string;
  min_subtotal: string;
  start: string;
  end: string;
  is_active: boolean;
  apply_mode: DiscountApplyMode;
  days: number;
  from: string;
  to: string;
}

function toDraft(
  rule: DiscountRule | null,
  currency: Parameters<typeof toDecimalString>[1],
): Draft {
  if (!rule) {
    return {
      id: null,
      name: '',
      kind: 'percentage',
      value: '',
      scope: 'order',
      target_id: '',
      min_subtotal: '',
      start: '',
      end: '',
      is_active: true,
      apply_mode: 'automatic',
      days: EVERY_DAY,
      from: '',
      to: '',
    };
  }
  return {
    id: rule.id,
    name: rule.name,
    kind: rule.kind,
    value:
      rule.kind === 'percentage'
        ? formatPercent(rule.value)
        : toDecimalString(rule.value, currency),
    scope: rule.scope,
    target_id: rule.target_id ?? '',
    min_subtotal: rule.min_subtotal ? toDecimalString(rule.min_subtotal, currency) : '',
    start: timestampToDate(rule.starts_at),
    end: timestampToDate(rule.ends_at, 1),
    is_active: rule.is_active,
    apply_mode: rule.apply_mode ?? 'manual',
    days: rule.days_mask ?? EVERY_DAY,
    from: rule.time_from === null ? '' : minutesToTime(rule.time_from),
    to: rule.time_to === null ? '' : minutesToTime(rule.time_to % 1440),
  };
}

function Editor({ initial, onClose }: { initial: Draft; onClose: () => void }) {
  const { t } = useTranslation();
  const { currency } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const names = dayNames(locale);
  const products = useProducts({ limit: 1000 });
  const categories = useCategories();
  const save = useSaveDiscountRule();
  const [draft, setDraft] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Draft>) => {
    setDraft({ ...draft, ...patch });
  };

  const build = (): DiscountRuleInput | string => {
    let value: number | null;
    if (draft.kind === 'percentage') {
      value = parsePercent(draft.value);
      if (value === null) return t('discounts.errors.percent');
    } else {
      try {
        value = parseDecimalString(draft.value, currency);
      } catch {
        return t('discounts.errors.amount');
      }
      if (value <= 0) return t('discounts.errors.amount');
    }
    let minSubtotal: number | null = null;
    if (draft.min_subtotal.trim()) {
      try {
        minSubtotal = parseDecimalString(draft.min_subtotal, currency);
      } catch {
        return t('discounts.errors.amount');
      }
    }
    const from = draft.from ? timeToMinutes(draft.from) : null;
    const to = draft.to ? timeToMinutes(draft.to, true) : null;
    if ((draft.from && from === null) || (draft.to && to === null)) {
      return t('discounts.errors.time');
    }
    if (draft.scope !== 'order' && !draft.target_id) return t('discounts.errors.target');
    if (draft.days === 0) return t('discounts.errors.days');
    return {
      id: draft.id,
      name: draft.name.trim(),
      kind: draft.kind,
      value,
      scope: draft.scope,
      target_id: draft.scope === 'order' ? null : (draft.target_id as DiscountRule['id']),
      min_subtotal: minSubtotal,
      starts_at: dateToTimestamp(draft.start),
      ends_at: dateToTimestamp(draft.end, 1),
      is_active: draft.is_active,
      apply_mode: draft.apply_mode,
      days_mask: draft.days === EVERY_DAY ? null : draft.days,
      time_from: from,
      time_to: to,
    };
  };

  return (
    <Modal open wide title={draft.id ? t('discounts.edit') : t('discounts.new')} onClose={onClose}>
      <form
        className="discount-form"
        onSubmit={(e) => {
          e.preventDefault();
          const input = build();
          if (typeof input === 'string') {
            setError(input);
            return;
          }
          setError(null);
          save.mutate(input, { onSuccess: onClose });
        }}
      >
        <label className="field">
          <span>{t('discounts.name')}</span>
          <input
            required
            maxLength={80}
            value={draft.name}
            placeholder={t('discounts.namePlaceholder')}
            onChange={(e) => {
              set({ name: e.target.value });
            }}
          />
        </label>

        <fieldset className="segmented">
          <legend>{t('discounts.how')}</legend>
          {(['automatic', 'manual'] as const).map((mode) => (
            <label key={mode} className="segmented__option">
              <input
                type="radio"
                name="mode"
                checked={draft.apply_mode === mode}
                onChange={() => {
                  set({ apply_mode: mode });
                }}
              />
              <strong>{t(`discounts.modes.${mode}`)}</strong>
              <span className="muted small">{t(`discounts.modes.${mode}Help`)}</span>
            </label>
          ))}
        </fieldset>

        <div className="form-grid">
          <label className="field">
            <span>{t('discounts.kind')}</span>
            <select
              value={draft.kind}
              onChange={(e) => {
                set({ kind: e.target.value === 'fixed_amount' ? 'fixed_amount' : 'percentage' });
              }}
            >
              <option value="percentage">{t('discounts.kinds.percentage')}</option>
              <option value="fixed_amount">
                {t('discounts.kinds.fixed_amount', { currency })}
              </option>
            </select>
          </label>
          <label className="field">
            <span>
              {draft.kind === 'percentage'
                ? t('discounts.percent')
                : t('discounts.amount', { currency })}
            </span>
            <input
              required
              dir="ltr"
              inputMode="decimal"
              value={draft.value}
              placeholder={draft.kind === 'percentage' ? '10' : '1.000'}
              onChange={(e) => {
                set({ value: e.target.value });
              }}
            />
          </label>
          <label className="field">
            <span>{t('discounts.scope')}</span>
            <select
              value={draft.scope}
              onChange={(e) => {
                const v = e.target.value;
                set({
                  scope: v === 'product' || v === 'category' ? v : 'order',
                  target_id: '',
                });
              }}
            >
              <option value="order">{t('discounts.scopes.order')}</option>
              <option value="product">{t('discounts.scopes.product')}</option>
              <option value="category">{t('discounts.scopes.category')}</option>
            </select>
          </label>
          {draft.scope !== 'order' && (
            <label className="field">
              <span>{t(`discounts.scopes.${draft.scope}`)}</span>
              <select
                required
                value={draft.target_id}
                onChange={(e) => {
                  set({ target_id: e.target.value });
                }}
              >
                <option value="">—</option>
                {draft.scope === 'product'
                  ? products.data?.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))
                  : categories.data?.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
              </select>
            </label>
          )}
          <label className="field">
            <span>{t('discounts.minSpend', { currency })}</span>
            <input
              dir="ltr"
              inputMode="decimal"
              value={draft.min_subtotal}
              placeholder={t('discounts.optional')}
              onChange={(e) => {
                set({ min_subtotal: e.target.value });
              }}
            />
          </label>
        </div>

        <h3>{t('discounts.when')}</h3>
        <div className="chips">
          {names.map((name, i) => (
            <button
              key={name}
              type="button"
              className="chip"
              aria-pressed={(draft.days & (1 << i)) !== 0}
              onClick={() => {
                set({ days: draft.days ^ (1 << i) });
              }}
            >
              {name}
            </button>
          ))}
          <span className="chips__sep" />
          {(
            [
              ['everyDay', EVERY_DAY],
              ['weekdays', WEEKDAYS],
              ['weekend', WEEKEND],
            ] as const
          ).map(([key, mask]) => (
            <button
              key={key}
              type="button"
              className="link-button"
              onClick={() => {
                set({ days: mask });
              }}
            >
              {t(`discounts.${key}`)}
            </button>
          ))}
        </div>
        <div className="form-grid">
          <label className="field">
            <span>{t('discounts.from')}</span>
            <input
              type="time"
              dir="ltr"
              value={draft.from}
              onChange={(e) => {
                set({ from: e.target.value });
              }}
            />
          </label>
          <label className="field">
            <span>{t('discounts.to')}</span>
            <input
              type="time"
              dir="ltr"
              value={draft.to}
              onChange={(e) => {
                set({ to: e.target.value });
              }}
            />
          </label>
          <label className="field">
            <span>{t('discounts.startDate')}</span>
            <input
              type="date"
              dir="ltr"
              value={draft.start}
              onChange={(e) => {
                set({ start: e.target.value });
              }}
            />
          </label>
          <label className="field">
            <span>{t('discounts.endDate')}</span>
            <input
              type="date"
              dir="ltr"
              value={draft.end}
              onChange={(e) => {
                set({ end: e.target.value });
              }}
            />
          </label>
        </div>
        <p className="muted small">{t('discounts.whenHelp')}</p>

        <label className="check">
          <input
            type="checkbox"
            checked={draft.is_active}
            onChange={(e) => {
              set({ is_active: e.target.checked });
            }}
          />
          {t('discounts.active')}
        </label>
        {(error ?? save.error) && (
          <p role="alert" className="error-text">
            {error ?? save.error?.message}
          </p>
        )}
        <div className="row">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button type="submit" className="button button--primary" disabled={save.isPending}>
            {t('common.save')}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function RuleRow({ view, onEdit }: { view: DiscountRuleView; onEdit: () => void }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const remove = useDeleteDiscountRule();
  const rule = view.rule;
  const value = rule.kind === 'percentage' ? `${formatPercent(rule.value)}%` : format(rule.value);
  const target =
    rule.scope === 'order'
      ? t('discounts.scopes.order')
      : `${t(`discounts.scopes.${rule.scope}`)}: ${view.target_name ?? '?'}`;
  const days = daysSummary(rule.days_mask, dayNames(locale));
  const hours =
    rule.time_from !== null || rule.time_to !== null
      ? `${minutesToTime(rule.time_from ?? 0)}–${minutesToTime(rule.time_to ?? 1440)}`
      : null;
  const when = [days, hours].filter(Boolean).join(' · ') || t('discounts.always');
  const status = !rule.is_active ? 'off' : view.live ? 'live' : 'waiting';

  return (
    <tr>
      <td>
        <strong>{rule.name}</strong>
        {rule.min_subtotal ? (
          <div className="muted small">
            {t('discounts.minSpendShort', { amount: format(rule.min_subtotal) })}
          </div>
        ) : null}
      </td>
      <td>{target}</td>
      <td className="num" dir="ltr">
        {value}
      </td>
      <td>{when}</td>
      <td>
        <span className="badge">{t(`discounts.modes.${rule.apply_mode ?? 'manual'}`)}</span>
      </td>
      <td>
        <span
          className={
            status === 'live' ? 'badge badge--ok' : status === 'off' ? 'badge badge--warn' : 'badge'
          }
        >
          {t(`discounts.status.${status}`)}
        </span>
      </td>
      <td className="actions">
        <button type="button" className="link-button" onClick={onEdit}>
          {t('common.edit')}
        </button>
        <button
          type="button"
          className="link-button danger"
          disabled={remove.isPending}
          onClick={() => {
            if (window.confirm(t('discounts.confirmDelete', { name: rule.name }))) {
              remove.mutate(rule.id);
            }
          }}
        >
          {t('common.delete')}
        </button>
      </td>
    </tr>
  );
}

/** Owner: discount rules — promotions that apply by themselves, and ones a manager applies. */
export function DiscountsScreen() {
  const { t } = useTranslation();
  const { currency } = useMoney();
  const rules = useDiscountRules();
  const [editing, setEditing] = useState<Draft | null>(null);

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('discounts.title')}</h1>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            setEditing(toDraft(null, currency));
          }}
        >
          + {t('discounts.new')}
        </button>
      </header>
      <p className="muted">{t('discounts.help')}</p>
      <section className="card">
        {rules.data?.length === 0 ? (
          <p className="muted">{t('discounts.none')}</p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>{t('discounts.name')}</th>
                <th>{t('discounts.scope')}</th>
                <th className="num">{t('discounts.value')}</th>
                <th>{t('discounts.when')}</th>
                <th>{t('discounts.how')}</th>
                <th>{t('discounts.statusTitle')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rules.data?.map((view) => (
                <RuleRow
                  key={view.rule.id}
                  view={view}
                  onEdit={() => {
                    setEditing(toDraft(view.rule, currency));
                  }}
                />
              ))}
            </tbody>
          </table>
        )}
      </section>
      {editing && (
        <Editor
          initial={editing}
          onClose={() => {
            setEditing(null);
          }}
        />
      )}
    </div>
  );
}
