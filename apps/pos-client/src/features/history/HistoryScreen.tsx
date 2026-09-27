import type { PaymentMethod, SaleReceipt, Session, TransactionKind, Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { usePrintReceipt, useTransaction, useTransactions } from '../../ipc/queries';
import { formatDateTime, presetRange, type RangePreset } from '../../lib/dates';
import { formatQuantity, useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';
import { useUiStore } from '../../stores/ui';
import { ReceiptDialog } from '../sell/ReceiptDialog';
import { RefundDialog, VoidDialog } from './RefundDialog';

type Range = RangePreset | 'all';
const RANGES: readonly Range[] = ['today', 'yesterday', 'last7', 'last30', 'all'];
const KINDS: readonly (TransactionKind | null)[] = [null, 'sale', 'refund', 'void'];
const PAGE = 50;

function KindBadge({ kind }: { kind: TransactionKind }) {
  const { t } = useTranslation();
  return <span className={`badge badge--${kind}`}>{t(`history.kinds.${kind}`)}</span>;
}

function Detail({
  id,
  session,
  onOpen,
  onReceipt,
}: {
  id: Uuid;
  session: Session;
  onOpen: (id: Uuid) => void;
  onReceipt: (receipt: SaleReceipt) => void;
}) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const detail = useTransaction(id);
  const print = usePrintReceipt();
  const [dialog, setDialog] = useState<'refund' | 'void' | null>(null);
  const d = detail.data;
  if (!d) return <aside className="history__detail muted">{detail.error?.message ?? '…'}</aside>;
  const r = d.receipt;

  return (
    <aside className="history__detail">
      <header className="history__head">
        <div>
          <h2>{r.receipt_number}</h2>
          <p className="muted small">
            {formatDateTime(r.issued_at, locale)} · {d.summary.cashier_name}
            {d.summary.approved_by_name &&
              ` · ${t('history.approvedBy', { name: d.summary.approved_by_name })}`}
          </p>
        </div>
        <KindBadge kind={d.summary.kind} />
      </header>
      {d.summary.original_id && (
        <button
          type="button"
          className="link-button"
          onClick={() => {
            if (d.summary.original_id) onOpen(d.summary.original_id);
          }}
        >
          {t('history.ofSale', { number: d.summary.original_receipt_number ?? '' })}
        </button>
      )}
      {d.summary.notes && <p className="small">“{d.summary.notes}”</p>}
      <ul className="history__lines">
        {r.lines.map((line, i) => {
          const view = d.lines[i];
          return (
            <li key={`${line.name}-${String(i)}`}>
              <div className="row">
                <span className="grow">
                  {formatQuantity(line.quantity_milli)} × {line.name}
                </span>
                <span>{format(line.line_total)}</span>
              </div>
              {line.modifiers.length > 0 && (
                <p className="muted small">{line.modifiers.map((m) => m.name).join(', ')}</p>
              )}
              {view && view.reversed_quantity_milli > 0 && (
                <p className="small tone--bad">
                  {t('history.backQty', { qty: formatQuantity(view.reversed_quantity_milli) })}
                </p>
              )}
            </li>
          );
        })}
      </ul>
      <dl className="summary">
        {r.discount_total !== 0 && (
          <>
            <dt>{t('sell.discount')}</dt>
            <dd>{format(-Math.abs(r.discount_total))}</dd>
          </>
        )}
        <dt className="summary__total">{t('sell.total')}</dt>
        <dd className="summary__total">{format(r.total)}</dd>
        {r.payments.map((p, i) => (
          <Payment key={`${p.method}-${String(i)}`} method={p.method} amount={p.amount} />
        ))}
      </dl>
      {d.reversals.length > 0 && (
        <section className="stack">
          <h3>{t('history.reversals')}</h3>
          <ul className="history__reversals">
            {d.reversals.map((rev) => (
              <li key={rev.id}>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    onOpen(rev.id);
                  }}
                >
                  <KindBadge kind={rev.kind} /> {rev.receipt_number} ·{' '}
                  {formatDateTime(rev.occurred_at, locale)}
                </button>
                <span>{format(rev.total)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {print.error && (
        <p role="alert" className="error-text">
          {print.error.message}
        </p>
      )}
      {print.data && (
        <p className={print.data.printed ? 'ok-text' : 'muted'}>
          {print.data.printed ? t('receipt.printed') : t('receipt.queued')}
        </p>
      )}
      <div className="row row--wrap">
        <button
          type="button"
          className="button"
          disabled={print.isPending}
          onClick={() => {
            print.mutate(r.transaction_id);
          }}
        >
          {t('receipt.printCopy')}
        </button>
        {can(session, 'sale.refund') && d.can_refund && (
          <button
            type="button"
            className="button button--danger"
            onClick={() => {
              setDialog('refund');
            }}
          >
            {t('history.refund')}
          </button>
        )}
        {can(session, 'sale.void') && d.summary.kind === 'sale' && (
          <button
            type="button"
            className="button button--danger"
            disabled={d.void_blocker !== null}
            title={d.void_blocker ?? undefined}
            onClick={() => {
              setDialog('void');
            }}
          >
            {t('history.void')}
          </button>
        )}
      </div>
      {d.summary.kind === 'sale' && d.void_blocker && can(session, 'sale.void') && (
        <p className="muted small">{d.void_blocker}</p>
      )}
      {dialog === 'refund' && (
        <RefundDialog
          detail={d}
          onClose={() => {
            setDialog(null);
          }}
          onDone={(receipt) => {
            setDialog(null);
            onReceipt(receipt);
          }}
        />
      )}
      {dialog === 'void' && (
        <VoidDialog
          detail={d}
          onClose={() => {
            setDialog(null);
          }}
          onDone={(receipt) => {
            setDialog(null);
            onReceipt(receipt);
          }}
        />
      )}
    </aside>
  );
}

function Payment({ method, amount }: { method: PaymentMethod; amount: number }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  return (
    <>
      <dt>{t(`pay.methods.${method}`)}</dt>
      <dd>{format(amount)}</dd>
    </>
  );
}

/** Sales, refunds and voids of the shop; reprint, refund, void. */
export function HistoryScreen({ session }: { session: Session }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const [range, setRange] = useState<Range>('today');
  const [kind, setKind] = useState<TransactionKind | null>(null);
  const [search, setSearch] = useState('');
  const [limit, setLimit] = useState(PAGE);
  const [selected, setSelected] = useState<Uuid | null>(null);
  const [receipt, setReceipt] = useState<SaleReceipt | null>(null);
  // Fixed while the screen is open, so the query key stays stable.
  const [dates] = useState(() => new Date());
  const bounds = range === 'all' ? { from: null, to: null } : presetRange(range, dates);
  const list = useTransactions({
    ...bounds,
    kind,
    search: search.trim() || null,
    limit,
  });
  const rows = list.data ?? [];

  return (
    <div className="history">
      <section className="history__list">
        <div className="history__filters">
          <input
            className="search"
            type="search"
            placeholder={t('history.search')}
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              setLimit(PAGE);
            }}
          />
          <div className="row row--wrap">
            {RANGES.map((r) => (
              <button
                key={r}
                type="button"
                className="chip"
                aria-pressed={range === r}
                onClick={() => {
                  setRange(r);
                  setLimit(PAGE);
                }}
              >
                {t(`ranges.${r}`)}
              </button>
            ))}
            <span className="grow" />
            {KINDS.map((k) => (
              <button
                key={String(k)}
                type="button"
                className="chip"
                aria-pressed={kind === k}
                onClick={() => {
                  setKind(k);
                  setLimit(PAGE);
                }}
              >
                {k ? t(`history.kinds.${k}`) : t('history.allKinds')}
              </button>
            ))}
          </div>
        </div>
        <div className="history__scroll">
          <table className="table">
            <thead>
              <tr>
                <th>{t('history.time')}</th>
                <th>{t('history.receipt')}</th>
                <th>{t('history.cashier')}</th>
                <th className="num">{t('sell.total')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 && (
                <tr>
                  <td colSpan={4} className="muted">
                    {list.isFetching ? '…' : t('history.empty')}
                  </td>
                </tr>
              )}
              {rows.map((row) => (
                <tr
                  key={row.id}
                  className={
                    row.id === selected ? 'table__link table__link--selected' : 'table__link'
                  }
                  tabIndex={0}
                  onClick={() => {
                    setSelected(row.id);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') setSelected(row.id);
                  }}
                >
                  <td>{formatDateTime(row.occurred_at, locale)}</td>
                  <td>
                    {row.receipt_number} {row.kind !== 'sale' && <KindBadge kind={row.kind} />}
                    {row.kind === 'sale' && row.reversed_total > 0 && (
                      <span className="badge badge--refund">
                        {t('history.reversedBy', { amount: format(row.reversed_total) })}
                      </span>
                    )}
                    {row.table_label && <span className="muted small"> · {row.table_label}</span>}
                  </td>
                  <td>{row.cashier_name}</td>
                  <td className={`num ${row.total < 0 ? 'tone--bad' : ''}`}>{format(row.total)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length >= limit && (
            <button
              type="button"
              className="button button--block"
              onClick={() => {
                setLimit(limit + PAGE);
              }}
            >
              {t('common.more')}
            </button>
          )}
        </div>
      </section>
      {selected ? (
        <Detail
          key={selected}
          id={selected}
          session={session}
          onOpen={setSelected}
          onReceipt={(r) => {
            setReceipt(r);
            setSelected(r.transaction_id);
          }}
        />
      ) : (
        <aside className="history__detail muted">{t('history.pick')}</aside>
      )}
      <ReceiptDialog
        receipt={receipt}
        session={session}
        doneLabel={t('common.done')}
        onNewSale={() => {
          setReceipt(null);
        }}
      />
    </div>
  );
}
