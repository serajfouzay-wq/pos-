import type { FireOutcome, OpenOrderView, SaleReceipt, Session, Uuid } from '@pos/shared';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { errorText, useToast } from '../../components/Toast';
import { useCancelOrder, useFireCourse, useQuote } from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';
import { CartPanel } from '../sell/CartPanel';
import { addLines, removeLine, setQuantity, toCartItems } from '../sell/lines';
import { PaymentDialog } from '../sell/PaymentDialog';
import { ProductGrid } from '../sell/ProductGrid';
import { useOrderSubmit } from '../sell/submit';
import { useBarcodeLookup } from '../sell/useBarcodeLookup';
import { useCatalogIndex } from '../sell/useCatalogIndex';
import { useProductPicker } from '../sell/useProductPicker';
import { useNoteEdit } from '../sell/useNoteEdit';
import { useWeightEdit } from '../sell/useWeightEdit';
import { SplitBillDialog } from './SplitBillDialog';
import { useOrderEditor } from './useOrderEditor';

const COURSES = [1, 2, 3, 4] as const;

interface Props {
  session: Session;
  order: OpenOrderView;
  /** Restaurant: course numbers and sending to the kitchen. */
  courses: boolean;
  backLabel: string;
  onBack: () => void;
  onPaid: (receipt: SaleReceipt) => void;
}

type Paying = { lineIds: Uuid[] | null; total: number } | null;

/** A cafe tab or a restaurant table: add, send, split and pay. */
export function OrderEditor({ session, order, courses, backLabel, onBack, onPaid }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const index = useCatalogIndex();
  const editor = useOrderEditor(order, index);
  const { lines, meta, change, dirty } = editor;
  const toast = useToast();
  const fire = useFireCourse();
  const cancel = useCancelOrder();
  const [course, setCourse] = useState<number | null>(courses ? 1 : null);
  const [ticket, setTicket] = useState<FireOutcome | null>(null);
  const [splitting, setSplitting] = useState(false);
  const [paying, setPaying] = useState<Paying>(null);
  const [confirmCancel, setConfirmCancel] = useState(false);

  const request = useMemo(
    () => (lines.length ? { items: toCartItems(lines), discount_rule_ids: [] } : null),
    [lines],
  );
  const quote = useQuote(request);
  const submit = useOrderSubmit(order.id, paying?.lineIds ?? null);
  const picker = useProductPicker({
    menu: index.menu,
    products: index.products,
    course,
    allowNote: true,
    onAdd: (added) => {
      change(addLines(lines, added));
    },
  });
  const weight = useWeightEdit((lineId, milli) => {
    change(setQuantity(lines, lineId, milli));
  });
  const note = useNoteEdit((lineId, text) => {
    change(lines.map((l) => (l.line_id === lineId ? { ...l, note: text } : l)));
  });
  useBarcodeLookup(
    picker.pick,
    toast.show,
    !paying && !splitting && !ticket && !picker.busy && !weight.busy && !note.busy,
  );

  const unfiredCourses = [
    ...new Set(lines.filter((l) => l.fired_at === null).map((l) => l.course)),
  ].sort((a, b) => (a ?? 0) - (b ?? 0));
  const unfired = lines.filter((l) => l.fired_at === null).length;

  const send = (which: number | null) => {
    fire.mutate(
      { order, course: which },
      {
        onSuccess: (outcome) => {
          if (outcome.printed) toast.show(t('orders.sentToKitchen'));
          else setTicket(outcome);
        },
      },
    );
  };

  const title = order.table_label
    ? t('orders.table', { label: order.table_label })
    : (order.label ?? t('orders.tab'));
  const error = editor.error ?? fire.error?.message ?? cancel.error?.message ?? null;

  return (
    <div className="sell sell--with-bar">
      <div className="order-bar">
        <button type="button" className="button" onClick={onBack}>
          {backLabel}
        </button>
        <strong className="order-bar__title">{title}</strong>
        <div className="stepper" dir="ltr" aria-label={t('orders.guests')}>
          <button
            type="button"
            disabled={meta.guests <= (order.table_id ? 1 : 0)}
            onClick={() => {
              change(lines, { ...meta, guests: meta.guests - 1 });
            }}
            aria-label={t('sell.less')}
          >
            −
          </button>
          <span>{t('orders.guestCount', { count: meta.guests })}</span>
          <button
            type="button"
            disabled={meta.guests >= 99}
            onClick={() => {
              change(lines, { ...meta, guests: meta.guests + 1 });
            }}
            aria-label={t('sell.more')}
          >
            +
          </button>
        </div>
        {courses && (
          <div className="order-bar__courses" role="group" aria-label={t('orders.newItemsCourse')}>
            <span className="muted small">{t('orders.newItemsCourse')}</span>
            {COURSES.map((c) => (
              <button
                key={c}
                type="button"
                className="chip"
                aria-pressed={course === c}
                onClick={() => {
                  setCourse(c);
                }}
              >
                {c}
              </button>
            ))}
          </div>
        )}
        {dirty && <span className="muted small">{t('orders.saving')}</span>}
        <button
          type="button"
          className="button button--danger order-bar__end"
          disabled={dirty || cancel.isPending}
          onClick={() => {
            setConfirmCancel(true);
          }}
        >
          {t('orders.cancel')}
        </button>
        {error && (
          <div className="banner banner--warning order-error" role="alert">
            {error}
            <button
              type="button"
              className="link-button"
              onClick={() => {
                editor.clearError();
                fire.reset();
                cancel.reset();
              }}
            >
              {t('common.done')}
            </button>
          </div>
        )}
      </div>
      <ProductGrid
        onPick={picker.pick}
        combos={index.menu?.combos ?? []}
        onPickCombo={picker.pickCombo}
      />
      <CartPanel
        title={title}
        lines={lines}
        quote={quote.data}
        quoteError={quote.error?.message}
        quoting={quote.isFetching}
        byCourse={courses}
        onSetQuantity={(lineId, milli) => {
          change(setQuantity(lines, lineId, milli));
        }}
        onRemove={(lineId) => {
          change(removeLine(lines, lineId));
        }}
        onEditWeight={weight.edit}
        onEditNote={note.edit}
        actions={
          <>
            {courses &&
              unfiredCourses.map((c) => (
                <button
                  key={String(c)}
                  type="button"
                  className="button"
                  disabled={dirty || fire.isPending}
                  onClick={() => {
                    send(c);
                  }}
                >
                  {c === null ? t('orders.sendUncoursed') : t('orders.sendCourse', { n: c })}
                </button>
              ))}
            {courses && unfired > 0 && unfiredCourses.length > 1 && (
              <button
                type="button"
                className="button"
                disabled={dirty || fire.isPending}
                onClick={() => {
                  send(null);
                }}
              >
                {t('orders.sendAll', { count: unfired })}
              </button>
            )}
            {lines.length > 1 && (
              <button
                type="button"
                className="button"
                disabled={dirty}
                onClick={() => {
                  setSplitting(true);
                }}
              >
                {t('orders.split')}
              </button>
            )}
          </>
        }
        payDisabled={dirty}
        onPay={() => {
          if (quote.data) setPaying({ lineIds: null, total: quote.data.total });
        }}
      />
      {picker.dialogs}
      {weight.dialog}
      {note.dialog}
      {splitting && (
        <SplitBillDialog
          order={order}
          lines={lines}
          onClose={() => {
            setSplitting(false);
          }}
          onPay={(lineIds, total) => {
            setSplitting(false);
            setPaying({ lineIds, total });
          }}
        />
      )}
      {paying && (
        <PaymentDialog
          open
          total={paying.total}
          guests={paying.lineIds ? 0 : order.guests}
          submit={submit}
          onClose={() => {
            setPaying(null);
          }}
          onComplete={(receipt) => {
            setPaying(null);
            onPaid(receipt);
          }}
        />
      )}
      <Modal
        open={ticket !== null}
        title={t('orders.ticketTitle')}
        onClose={() => {
          setTicket(null);
        }}
      >
        <div className="stack">
          <p className="muted">
            {ticket?.print_error
              ? t('orders.ticketNotPrinted', { error: ticket.print_error })
              : t('orders.noKitchenPrinter')}
          </p>
          <pre className="ticket-text" dir="auto">
            {ticket?.ticket_text}
          </pre>
          <button
            type="button"
            className="button button--primary"
            onClick={() => {
              setTicket(null);
            }}
          >
            {t('common.done')}
          </button>
        </div>
      </Modal>
      <Modal
        open={confirmCancel}
        title={t('orders.cancelTitle', { name: title })}
        onClose={() => {
          setConfirmCancel(false);
        }}
      >
        <div className="stack">
          <p>
            {lines.some((l) => l.fired_at) && !can(session, 'sale.void')
              ? t('orders.cancelNeedsManager')
              : lines.length > 0
                ? t('orders.cancelBody', {
                    count: lines.length,
                    amount: quote.data ? format(quote.data.total) : '…',
                  })
                : t('orders.cancelEmpty')}
          </p>
          <div className="row row--end">
            <button
              type="button"
              className="button"
              onClick={() => {
                setConfirmCancel(false);
              }}
            >
              {t('common.back')}
            </button>
            <button
              type="button"
              className="button button--danger"
              disabled={cancel.isPending}
              onClick={() => {
                cancel.mutate(order, {
                  onSuccess: onBack,
                  onError: (e) => {
                    toast.show(errorText(e));
                  },
                  onSettled: () => {
                    setConfirmCancel(false);
                  },
                });
              }}
            >
              {t('orders.cancelConfirm')}
            </button>
          </div>
        </div>
      </Modal>
      {toast.node}
    </div>
  );
}
