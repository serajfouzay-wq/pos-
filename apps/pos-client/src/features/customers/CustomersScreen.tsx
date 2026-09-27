import {
  CURRENCIES,
  parseDecimalString,
  toDecimalString,
  type Customer,
  type LoyaltySettings,
  type Session,
  type Uuid,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import {
  useAdjustPoints,
  useCustomer,
  useCustomers,
  useDeleteCustomer,
  useLoyaltyProgram,
  useSaveCustomer,
  useSaveLoyaltyProgram,
} from '../../ipc/queries';
import { formatDateTime } from '../../lib/dates';
import { useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';
import { useUiStore } from '../../stores/ui';

/** Owner: the loyalty programme (shared by every till of the shop). */
function ProgramCard() {
  const { t } = useTranslation();
  const { format, currency } = useMoney();
  const program = useLoyaltyProgram();
  const save = useSaveLoyaltyProgram();
  const [draft, setDraft] = useState<(LoyaltySettings & { value: string }) | null>(null);
  const data = program.data;
  if (!data) return null;
  if (!data.available) {
    return (
      <section className="card">
        <h2>{t('customers.program.title')}</h2>
        <p className="muted">{t('customers.program.unavailable')}</p>
      </section>
    );
  }
  // The field holds a plain decimal (never the localised display text) so it parses back.
  const current = draft ?? {
    ...data.settings,
    value: toDecimalString(data.settings.point_value, currency),
  };
  const unit = 10 ** CURRENCIES[currency].exponent;
  const set = (patch: Partial<typeof current>) => {
    setDraft({ ...current, ...patch });
  };
  const int = (text: string) => Number(text.replace(/\D/g, '') || '0');
  let pointValue: number | null = null;
  try {
    pointValue = parseDecimalString(current.value || '0', currency);
  } catch {
    pointValue = null;
  }

  return (
    <section className="card">
      <h2>{t('customers.program.title')}</h2>
      <form
        className="program"
        onSubmit={(e) => {
          e.preventDefault();
          if (pointValue === null) return;
          save.mutate(
            {
              enabled: current.enabled,
              points_per_unit: current.points_per_unit,
              point_value: pointValue,
              min_redeem_points: current.min_redeem_points,
              max_redeem_bps: current.max_redeem_bps,
            },
            {
              onSuccess: () => {
                setDraft(null);
              },
            },
          );
        }}
      >
        <label className="check">
          <input
            type="checkbox"
            checked={current.enabled}
            onChange={(e) => {
              set({ enabled: e.target.checked });
            }}
          />
          {t('customers.program.enabled')}
        </label>
        <label className="field">
          <span>{t('customers.program.pointsPerUnit', { amount: format(unit) })}</span>
          <input
            dir="ltr"
            inputMode="numeric"
            value={String(current.points_per_unit)}
            onChange={(e) => {
              set({ points_per_unit: int(e.target.value) });
            }}
          />
        </label>
        <label className="field">
          <span>{t('customers.program.pointValue')}</span>
          <input
            dir="ltr"
            inputMode="decimal"
            value={current.value}
            aria-invalid={pointValue === null}
            onChange={(e) => {
              set({ value: e.target.value });
            }}
          />
        </label>
        <label className="field">
          <span>{t('customers.program.minRedeem')}</span>
          <input
            dir="ltr"
            inputMode="numeric"
            value={String(current.min_redeem_points)}
            onChange={(e) => {
              set({ min_redeem_points: int(e.target.value) });
            }}
          />
        </label>
        <label className="field">
          <span>{t('customers.program.maxShare')}</span>
          <input
            dir="ltr"
            inputMode="numeric"
            value={String(current.max_redeem_bps / 100)}
            onChange={(e) => {
              set({ max_redeem_bps: Math.min(100, int(e.target.value)) * 100 });
            }}
          />
        </label>
        <p className="muted small program__example">
          {t('customers.program.example', {
            unit: format(unit),
            earn: t('customers.points', { count: current.points_per_unit }),
            value: format(data.settings.point_value),
          })}
        </p>
        {save.error && (
          <p role="alert" className="error-text">
            {save.error.message}
          </p>
        )}
        <div className="row row--end">
          {save.isSuccess && !draft && (
            <span className="ok-text">{t('customers.program.saved')}</span>
          )}
          <button
            type="submit"
            className="button button--primary"
            disabled={!draft || pointValue === null || save.isPending}
          >
            {t('common.save')}
          </button>
        </div>
      </form>
    </section>
  );
}

function EditCustomer({ customer, onClose }: { customer: Customer | null; onClose: () => void }) {
  const { t } = useTranslation();
  const save = useSaveCustomer();
  const [name, setName] = useState(customer?.display_name ?? '');
  const [phone, setPhone] = useState(customer?.phone ?? '');
  const [email, setEmail] = useState(customer?.email ?? '');
  const [notes, setNotes] = useState(customer?.notes ?? '');
  return (
    <Modal open title={customer?.display_name ?? t('customers.new')} onClose={onClose}>
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate(
            {
              id: customer?.id ?? null,
              display_name: name,
              phone: phone.trim() || null,
              email: email.trim() || null,
              notes: notes.trim() || null,
            },
            { onSuccess: onClose },
          );
        }}
      >
        <label className="field">
          <span>{t('customers.name')}</span>
          <input
            required
            autoFocus
            maxLength={120}
            value={name}
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </label>
        <label className="field">
          <span>{t('customers.phone')}</span>
          <input
            dir="ltr"
            inputMode="tel"
            maxLength={32}
            value={phone}
            onChange={(e) => {
              setPhone(e.target.value);
            }}
          />
        </label>
        <label className="field">
          <span>{t('customers.email')}</span>
          <input
            dir="ltr"
            type="email"
            maxLength={200}
            value={email}
            onChange={(e) => {
              setEmail(e.target.value);
            }}
          />
        </label>
        <label className="field">
          <span>{t('customers.notes')}</span>
          <textarea
            rows={3}
            maxLength={1000}
            value={notes}
            onChange={(e) => {
              setNotes(e.target.value);
            }}
          />
        </label>
        {save.error && (
          <p role="alert" className="error-text">
            {save.error.message}
          </p>
        )}
        <div className="row row--end">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={!name.trim() || save.isPending}
          >
            {t('common.save')}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function AdjustPoints({ customer, onClose }: { customer: Customer; onClose: () => void }) {
  const { t } = useTranslation();
  const adjust = useAdjustPoints();
  const [delta, setDelta] = useState('');
  const [note, setNote] = useState('');
  const points = /^[+-]?\d{1,7}$/.test(delta.trim()) ? Number(delta.trim()) : 0;
  return (
    <Modal
      open
      title={t('customers.adjustTitle', { name: customer.display_name })}
      onClose={onClose}
    >
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          adjust.mutate(
            { customer_id: customer.id, points_delta: points, note },
            { onSuccess: onClose },
          );
        }}
      >
        <p className="muted">{t('customers.adjustHelp')}</p>
        <label className="field">
          <span>{t('customers.adjustDelta')}</span>
          <input
            dir="ltr"
            autoFocus
            inputMode="numeric"
            placeholder="+100"
            value={delta}
            onChange={(e) => {
              setDelta(e.target.value);
            }}
          />
        </label>
        <label className="field">
          <span>{t('customers.adjustNote')}</span>
          <input
            maxLength={200}
            value={note}
            onChange={(e) => {
              setNote(e.target.value);
            }}
          />
        </label>
        {adjust.error && (
          <p role="alert" className="error-text">
            {adjust.error.message}
          </p>
        )}
        <div className="row row--end">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={points === 0 || !note.trim() || adjust.isPending}
          >
            {t('customers.adjust')}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function Detail({ id, session, onGone }: { id: Uuid; session: Session; onGone: () => void }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const detail = useCustomer(id);
  const remove = useDeleteCustomer();
  const [dialog, setDialog] = useState<'edit' | 'adjust' | 'delete' | null>(null);
  const d = detail.data;
  if (!d) return <aside className="customers__detail muted">{detail.error?.message ?? '…'}</aside>;
  const c = d.customer;
  const manage = can(session, 'customer.manage');

  return (
    <aside className="customers__detail">
      <header className="customers__head">
        <div>
          <h2>{c.display_name}</h2>
          <p className="muted small">{[c.phone, c.email].filter(Boolean).join(' · ') || '—'}</p>
          {c.notes && <p className="small">“{c.notes}”</p>}
        </div>
        <div className="customers__points">
          <strong>{c.loyalty_points}</strong>
          <span className="muted small">{t('customers.points', { count: c.loyalty_points })}</span>
        </div>
      </header>
      <p className="muted small">
        {t('customers.visits', { count: d.visits })} ·{' '}
        {t('customers.spent', { amount: format(d.spent) })} ·{' '}
        {d.last_visit_at
          ? t('customers.lastVisit', { when: formatDateTime(d.last_visit_at, locale) })
          : t('customers.never')}
      </p>
      {manage && (
        <div className="row row--wrap">
          <button
            type="button"
            className="button"
            onClick={() => {
              setDialog('edit');
            }}
          >
            {t('common.edit')}
          </button>
          <button
            type="button"
            className="button"
            onClick={() => {
              setDialog('adjust');
            }}
          >
            {t('customers.adjust')}
          </button>
          <button
            type="button"
            className="button button--danger"
            onClick={() => {
              setDialog('delete');
            }}
          >
            {t('customers.delete')}
          </button>
        </div>
      )}
      <h3>{t('customers.ledger')}</h3>
      <table className="table">
        <tbody>
          {d.ledger.length === 0 && (
            <tr>
              <td className="muted">{t('customers.noLedger')}</td>
            </tr>
          )}
          {d.ledger.map((entry) => (
            <tr key={entry.id}>
              <td>{formatDateTime(entry.occurred_at, locale)}</td>
              <td>
                {t(`customers.reasons.${entry.reason}`)}
                {entry.receipt_number && (
                  <span className="muted small"> · {entry.receipt_number}</span>
                )}
              </td>
              <td className="muted small">{entry.user_name}</td>
              <td
                className={`num ${entry.points_delta < 0 ? 'tone--bad' : 'tone--good'}`}
                dir="ltr"
              >
                {entry.points_delta > 0 ? `+${String(entry.points_delta)}` : entry.points_delta}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {dialog === 'edit' && (
        <EditCustomer
          customer={c}
          onClose={() => {
            setDialog(null);
          }}
        />
      )}
      {dialog === 'adjust' && (
        <AdjustPoints
          customer={c}
          onClose={() => {
            setDialog(null);
          }}
        />
      )}
      <Modal
        open={dialog === 'delete'}
        title={t('customers.delete')}
        onClose={() => {
          setDialog(null);
        }}
      >
        <div className="stack">
          <p>{t('customers.deleteConfirm', { name: c.display_name })}</p>
          {remove.error && (
            <p role="alert" className="error-text">
              {remove.error.message}
            </p>
          )}
          <div className="row row--end">
            <button
              type="button"
              className="button"
              onClick={() => {
                setDialog(null);
              }}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="button button--danger"
              disabled={remove.isPending}
              onClick={() => {
                remove.mutate(c.id, { onSuccess: onGone });
              }}
            >
              {t('customers.delete')}
            </button>
          </div>
        </div>
      </Modal>
    </aside>
  );
}

/** Customers and their points; the programme's rules for the owner. */
export function CustomersScreen({ session }: { session: Session }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<Uuid | null>(null);
  const [adding, setAdding] = useState(false);
  // Search narrows it down; a long list is not browsed.
  const list = useCustomers({ query: query.trim(), limit: 100 });
  const rows = list.data ?? [];

  return (
    <div className="admin customers">
      <header className="admin__header">
        <h1>{t('customers.title')}</h1>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            setAdding(true);
          }}
        >
          + {t('customers.new')}
        </button>
      </header>
      {can(session, 'settings.manage') && <ProgramCard />}
      <div className="customers__body">
        <section className="customers__list">
          <input
            className="search"
            type="search"
            placeholder={t('customers.search')}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
          />
          <ul className="customer-list">
            {rows.length === 0 && (
              <li className="muted">{query ? t('customers.none') : t('customers.noneYet')}</li>
            )}
            {rows.map((c) => (
              <li key={c.id}>
                <button
                  type="button"
                  className="customer-list__item"
                  aria-current={c.id === selected ? 'true' : undefined}
                  onClick={() => {
                    setSelected(c.id);
                  }}
                >
                  <span className="grow">
                    <strong>{c.display_name}</strong>
                    {c.phone && (
                      <span className="muted small" dir="ltr">
                        {' '}
                        · {c.phone}
                      </span>
                    )}
                  </span>
                  <span className="badge">
                    {t('customers.points', { count: c.loyalty_points })}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>
        {selected ? (
          <Detail
            key={selected}
            id={selected}
            session={session}
            onGone={() => {
              setSelected(null);
            }}
          />
        ) : (
          <aside className="customers__detail muted">{t('customers.pick')}</aside>
        )}
      </div>
      {adding && (
        <EditCustomer
          customer={null}
          onClose={() => {
            setAdding(false);
          }}
        />
      )}
    </div>
  );
}
