import type { Receipt, SaleReceipt, Session } from '@pos/shared';
import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { usePrintReceipt } from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';

interface Props {
  receipt: SaleReceipt | null;
  session: Session;
  onNewSale: () => void;
  /** Label of the closing button (history: "Done"). */
  doneLabel?: string;
}

/** Points on a receipt: earned on a sale; returned / taken back on a reversal. */
export function usePointsText(): (receipt: Receipt) => string {
  const { t } = useTranslation();
  return (receipt) => {
    const points = receipt.loyalty;
    if (!points) return '';
    const parts =
      receipt.kind === 'sale'
        ? [t('customers.earns', { count: points.earned })]
        : [
            ...(points.redeemed > 0 ? [t('customers.returned', { count: points.redeemed })] : []),
            ...(points.earned > 0 ? [t('customers.takenBack', { count: points.earned })] : []),
          ];
    return [...parts, t('customers.balance', { count: points.balance })].join(' · ');
  };
}

export function ReceiptDialog({ receipt, session, onNewSale, doneLabel }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const print = usePrintReceipt();
  const pointsText = usePointsText();
  if (!receipt) return null;

  const printedNow = print.data?.printed ?? receipt.printed;
  const queued = print.data?.queued ?? receipt.print_queued;
  // First print is anyone's; a copy after a successful print is a reprint.
  const mayPrint = printedNow ? can(session, 'receipt.reprint') : can(session, 'receipt.print');

  return (
    <Modal
      open
      title={
        receipt.kind === 'sale'
          ? t('receipt.title', { number: receipt.receipt_number })
          : t(`receipt.${receipt.kind}Title`, { number: receipt.receipt_number })
      }
    >
      <div className="stack receipt-done">
        <motion.div
          key={receipt.transaction_id}
          className={`update-check receipt-done__check receipt-done__check--${receipt.kind}`}
          initial={{ scale: 0.4, opacity: 0, rotate: -30 }}
          animate={{ scale: 1, opacity: 1, rotate: 0 }}
          transition={{ type: 'spring', stiffness: 300, damping: 15 }}
          aria-hidden="true"
        >
          {receipt.kind === 'sale' ? '✓' : '↺'}
        </motion.div>
        {receipt.change_due > 0 ? (
          <>
            <span className="muted">{t('receipt.changeDue')}</span>
            <output className="receipt-done__change">{format(receipt.change_due)}</output>
          </>
        ) : (
          <output className="receipt-done__change">{format(receipt.total)}</output>
        )}
        {receipt.customer_name && (
          <motion.p
            className="receipt-done__customer"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.15 }}
          >
            <strong>{receipt.customer_name}</strong>
            {receipt.loyalty && <> · {pointsText(receipt)}</>}
            {receipt.member && (
              <>
                {' · '}
                {t('memberships.member', { plan: receipt.member.plan_name })}
              </>
            )}
          </motion.p>
        )}
        <ul className="status-list">
          <li className={printedNow ? 'ok' : 'warn'}>
            {printedNow
              ? t('receipt.printed')
              : queued
                ? t('receipt.queued')
                : t('receipt.notPrinted')}
          </li>
          {receipt.payments.some((p) => p.method === 'cash') && (
            <li className={receipt.drawer_opened ? 'ok' : 'warn'}>
              {receipt.drawer_opened ? t('receipt.drawerOpened') : t('receipt.drawerFailed')}
            </li>
          )}
        </ul>
        {print.error && (
          <p role="alert" className="error-text">
            {print.error.message}
          </p>
        )}
        <div className="row">
          {mayPrint && (
            <button
              type="button"
              className="button"
              disabled={print.isPending}
              onClick={() => {
                print.mutate(receipt.transaction_id);
              }}
            >
              {printedNow ? t('receipt.printCopy') : t('receipt.printNow')}
            </button>
          )}
          <button
            type="button"
            className="button button--primary grow"
            autoFocus
            onClick={() => {
              print.reset();
              onNewSale();
            }}
          >
            {doneLabel ?? t('receipt.newSale')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
