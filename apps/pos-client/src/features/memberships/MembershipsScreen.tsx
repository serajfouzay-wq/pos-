import {
  parseDecimalString,
  toDecimalString,
  type Customer,
  type MemberState,
  type MembershipPlan,
  type MembershipPlanInput,
  type Session,
  type Uuid,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import {
  useCancelMembership,
  useDeleteMembershipPlan,
  useGrantMembership,
  useMembers,
  useMembershipPlans,
  useSaveMembershipPlan,
} from '../../ipc/queries';
import { formatDate } from '../../lib/dates';
import { useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';
import { useUiStore } from '../../stores/ui';
import { CustomerPicker } from '../customers/CustomerPicker';
import { formatPercent, parsePercent } from '../discounts/schedule';

interface PlanDraft {
  id: Uuid | null;
  name: string;
  description: string;
  price: string;
  duration_days: string;
  discount: string;
  multiplier: string;
  is_active: boolean;
}

/** Common lengths offered as one tap. */
const DURATIONS = [30, 90, 180, 365] as const;
const MULTIPLIERS = ['1', '1.5', '2', '3'] as const;

function PlanEditor({ plan, onClose }: { plan: MembershipPlan | null; onClose: () => void }) {
  const { t } = useTranslation();
  const { currency } = useMoney();
  const save = useSaveMembershipPlan();
  const [draft, setDraft] = useState<PlanDraft>(() => ({
    id: plan?.id ?? null,
    name: plan?.name ?? '',
    description: plan?.description ?? '',
    price: plan ? toDecimalString(plan.price, currency) : '',
    duration_days: String(plan?.duration_days ?? 30),
    discount: plan && plan.discount_bps > 0 ? formatPercent(plan.discount_bps) : '',
    multiplier: plan ? formatPercent(plan.points_multiplier_bps / 100) : '1',
    is_active: plan?.is_active ?? true,
  }));
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<PlanDraft>) => {
    setDraft({ ...draft, ...patch });
  };

  const build = (): MembershipPlanInput | string => {
    let price: number;
    try {
      price = parseDecimalString(draft.price || '0', currency);
    } catch {
      return t('discounts.errors.amount');
    }
    const days = Number(draft.duration_days);
    if (!Number.isInteger(days) || days < 1 || days > 3660) return t('memberships.errors.days');
    const discount = draft.discount.trim() ? parsePercent(draft.discount) : 0;
    if (discount === null) return t('discounts.errors.percent');
    // "1.5" × → 15 000 bps (parsePercent reads it as 1.5%, ×100 = 150 → ×100).
    const multiplier = parsePercent(draft.multiplier);
    if (multiplier === null || multiplier > 1_000) return t('memberships.errors.multiplier');
    return {
      id: draft.id,
      name: draft.name.trim(),
      description: draft.description.trim() || null,
      price,
      duration_days: days,
      discount_bps: discount,
      points_multiplier_bps: multiplier * 100,
      color: null,
      is_active: draft.is_active,
    };
  };

  return (
    <Modal
      open
      wide
      title={plan ? t('memberships.editPlan') : t('memberships.newPlan')}
      onClose={onClose}
    >
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
        <div className="form-grid">
          <label className="field">
            <span>{t('memberships.planName')}</span>
            <input
              required
              maxLength={80}
              value={draft.name}
              placeholder={t('memberships.planPlaceholder')}
              onChange={(e) => {
                set({ name: e.target.value });
              }}
            />
          </label>
          <label className="field">
            <span>{t('memberships.price', { currency })}</span>
            <input
              required
              dir="ltr"
              inputMode="decimal"
              value={draft.price}
              onChange={(e) => {
                set({ price: e.target.value });
              }}
            />
          </label>
        </div>
        <label className="field">
          <span>{t('memberships.description')}</span>
          <input
            maxLength={500}
            value={draft.description}
            placeholder={t('discounts.optional')}
            onChange={(e) => {
              set({ description: e.target.value });
            }}
          />
        </label>
        <div className="field">
          <span>{t('memberships.duration')}</span>
          <div className="chips">
            {DURATIONS.map((d) => (
              <button
                key={d}
                type="button"
                className="chip"
                aria-pressed={draft.duration_days === String(d)}
                onClick={() => {
                  set({ duration_days: String(d) });
                }}
              >
                {t(`memberships.durations.${d}`)}
              </button>
            ))}
            <input
              className="port"
              dir="ltr"
              inputMode="numeric"
              aria-label={t('memberships.days')}
              value={draft.duration_days}
              onChange={(e) => {
                set({ duration_days: e.target.value.replace(/\D/g, '') });
              }}
            />
            <span className="muted small">{t('memberships.days')}</span>
          </div>
        </div>
        <div className="form-grid">
          <label className="field">
            <span>{t('memberships.discount')}</span>
            <input
              dir="ltr"
              inputMode="decimal"
              value={draft.discount}
              placeholder="0"
              onChange={(e) => {
                set({ discount: e.target.value });
              }}
            />
          </label>
          <div className="field">
            <span>{t('memberships.points')}</span>
            <div className="chips">
              {MULTIPLIERS.map((m) => (
                <button
                  key={m}
                  type="button"
                  className="chip"
                  aria-pressed={draft.multiplier === m}
                  onClick={() => {
                    set({ multiplier: m });
                  }}
                >
                  ×{m}
                </button>
              ))}
            </div>
          </div>
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.is_active}
            onChange={(e) => {
              set({ is_active: e.target.checked });
            }}
          />
          {t('memberships.onSale')}
        </label>
        <p className="muted small">{t('memberships.planHelp')}</p>
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

function GrantDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const plans = useMembershipPlans();
  const grant = useGrantMembership();
  const [customer, setCustomer] = useState<Customer | null>(null);
  const [picking, setPicking] = useState(true);
  const [planId, setPlanId] = useState<Uuid | ''>('');
  const [notes, setNotes] = useState('');
  return (
    <Modal open title={t('memberships.grant')} onClose={onClose}>
      <form
        className="discount-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (!customer || !planId) return;
          grant.mutate(
            { customer_id: customer.id, plan_id: planId, notes: notes.trim() || null },
            { onSuccess: onClose },
          );
        }}
      >
        <p className="muted">{t('memberships.grantHelp')}</p>
        <div className="row">
          <strong className="grow">{customer?.display_name ?? '—'}</strong>
          <button
            type="button"
            className="button"
            onClick={() => {
              setPicking(true);
            }}
          >
            {t('customers.pickTitle')}
          </button>
        </div>
        <label className="field">
          <span>{t('memberships.plan')}</span>
          <select
            required
            value={planId}
            onChange={(e) => {
              setPlanId(e.target.value as Uuid);
            }}
          >
            <option value="">—</option>
            {plans.data
              ?.filter((v) => v.plan.is_active)
              .map((v) => (
                <option key={v.plan.id} value={v.plan.id}>
                  {v.plan.name}
                </option>
              ))}
          </select>
        </label>
        <label className="field">
          <span>{t('customers.notes')}</span>
          <input
            maxLength={500}
            value={notes}
            onChange={(e) => {
              setNotes(e.target.value);
            }}
          />
        </label>
        {grant.error && (
          <p role="alert" className="error-text">
            {grant.error.message}
          </p>
        )}
        <div className="row">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={!customer || !planId || grant.isPending}
          >
            {t('memberships.grant')}
          </button>
        </div>
      </form>
      <CustomerPicker
        open={picking}
        onClose={() => {
          setPicking(false);
        }}
        onPick={(c) => {
          setPicking(false);
          setCustomer(c);
        }}
      />
    </Modal>
  );
}

const STATE_FILTERS: (MemberState | null)[] = ['active', 'upcoming', 'expired', null];

/** Membership plans and the customers holding them. */
export function MembershipsScreen({ session }: { session: Session }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const manage = can(session, 'customer.manage');
  const plans = useMembershipPlans();
  const [query, setQuery] = useState('');
  const [state, setState] = useState<MemberState | null>('active');
  const members = useMembers({ query, state, customer_id: null, limit: 200 });
  const deletePlan = useDeleteMembershipPlan();
  const cancel = useCancelMembership();
  const [editing, setEditing] = useState<MembershipPlan | null | undefined>(undefined);
  const [granting, setGranting] = useState(false);

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('memberships.title')}</h1>
        {manage && (
          <div className="row">
            <button
              type="button"
              className="button"
              onClick={() => {
                setGranting(true);
              }}
            >
              {t('memberships.grant')}
            </button>
            <button
              type="button"
              className="button button--primary"
              onClick={() => {
                setEditing(null);
              }}
            >
              + {t('memberships.newPlan')}
            </button>
          </div>
        )}
      </header>
      <p className="muted">{t('memberships.help')}</p>

      <section className="plans">
        {plans.data?.length === 0 && <p className="muted">{t('memberships.noPlans')}</p>}
        {plans.data?.map(({ plan, active_members }) => (
          <article key={plan.id} className={plan.is_active ? 'plan' : 'plan plan--off'}>
            <header className="row">
              <h2 className="grow">{plan.name}</h2>
              <strong>{format(plan.price)}</strong>
            </header>
            {plan.description && <p className="muted small">{plan.description}</p>}
            <ul className="plan__perks">
              <li>{t('memberships.lasts', { count: plan.duration_days })}</li>
              {plan.discount_bps > 0 && (
                <li>{t('memberships.off', { percent: formatPercent(plan.discount_bps) })}</li>
              )}
              {plan.points_multiplier_bps !== 10_000 && (
                <li>
                  {t('memberships.pointsTimes', {
                    times: formatPercent(plan.points_multiplier_bps / 100),
                  })}
                </li>
              )}
            </ul>
            <footer className="row">
              <span className="badge grow-none">
                {t('memberships.activeMembers', { count: active_members })}
              </span>
              {!plan.is_active && (
                <span className="badge badge--warn">{t('memberships.notOnSale')}</span>
              )}
              <span className="grow" />
              {manage && (
                <>
                  <button
                    type="button"
                    className="link-button"
                    onClick={() => {
                      setEditing(plan);
                    }}
                  >
                    {t('common.edit')}
                  </button>
                  <button
                    type="button"
                    className="link-button danger"
                    disabled={deletePlan.isPending}
                    onClick={() => {
                      if (window.confirm(t('memberships.confirmDeletePlan', { name: plan.name }))) {
                        deletePlan.mutate(plan.id);
                      }
                    }}
                  >
                    {t('common.delete')}
                  </button>
                </>
              )}
            </footer>
          </article>
        ))}
      </section>

      <section className="card">
        <div className="row">
          <h2 className="grow">{t('memberships.members')}</h2>
          <input
            className="search"
            value={query}
            placeholder={t('memberships.search')}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
          />
        </div>
        <div className="chips">
          {STATE_FILTERS.map((s) => (
            <button
              key={String(s)}
              type="button"
              className="chip"
              aria-pressed={state === s}
              onClick={() => {
                setState(s);
              }}
            >
              {t(`memberships.states.${s ?? 'all'}`)}
            </button>
          ))}
        </div>
        {members.data?.length === 0 ? (
          <p className="muted">{t('memberships.noMembers')}</p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>{t('customers.name')}</th>
                <th>{t('memberships.plan')}</th>
                <th>{t('memberships.card')}</th>
                <th>{t('memberships.period')}</th>
                <th>{t('discounts.statusTitle')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {members.data?.map((row) => (
                <tr key={row.membership.id}>
                  <td>
                    <strong>{row.customer_name}</strong>
                    {row.customer_phone && (
                      <div className="muted small" dir="ltr">
                        {row.customer_phone}
                      </div>
                    )}
                  </td>
                  <td>{row.plan_name}</td>
                  <td dir="ltr" className="mono">
                    {row.membership.card_number}
                  </td>
                  <td>
                    {formatDate(row.membership.starts_at, locale)} –{' '}
                    {formatDate(row.membership.ends_at, locale)}
                  </td>
                  <td>
                    <span
                      className={
                        row.state === 'active'
                          ? 'badge badge--ok'
                          : row.state === 'cancelled' || row.state === 'expired'
                            ? 'badge badge--warn'
                            : 'badge'
                      }
                    >
                      {t(`memberships.states.${row.state}`)}
                    </span>
                  </td>
                  <td className="actions">
                    {manage && (row.state === 'active' || row.state === 'upcoming') && (
                      <button
                        type="button"
                        className="link-button danger"
                        disabled={cancel.isPending}
                        onClick={() => {
                          if (
                            window.confirm(
                              t('memberships.confirmCancel', { name: row.customer_name }),
                            )
                          ) {
                            cancel.mutate(row.membership.id);
                          }
                        }}
                      >
                        {t('memberships.cancel')}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      {editing !== undefined && (
        <PlanEditor
          plan={editing}
          onClose={() => {
            setEditing(undefined);
          }}
        />
      )}
      {granting && (
        <GrantDialog
          onClose={() => {
            setGranting(false);
          }}
        />
      )}
    </div>
  );
}
