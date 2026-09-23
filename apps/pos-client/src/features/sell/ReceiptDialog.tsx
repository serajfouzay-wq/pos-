import type { SaleReceipt, Session } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { usePrintReceipt } from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';

interface Props {
  receipt: SaleReceipt | null;
  session: Session;
  onNewSale: () => void;
}

export function ReceiptDialog({ receipt, session, onNewSale }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const print = usePrintReceipt();
  if (!receipt) return null;

  const printedNow = print.data?.printed ?? receipt.printed;
  const queued = print.data?.queued ?? !receipt.printed;
  // First print is anyone's; a copy after a successful print is a reprint.
  const mayPrint = printedNow ? can(session, 'receipt.reprint') : can(session, 'receipt.print');

  return (
    <Modal open title={t('receipt.title', { number: receipt.receipt_number })}>
      <div className="stack receipt-done">
        {receipt.change_due > 0 ? (
          <>
            <span className="muted">{t('receipt.changeDue')}</span>
            <output className="receipt-done__change">{format(receipt.change_due)}</output>
          </>
        ) : (
          <output className="receipt-done__change">{format(receipt.total)}</output>
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
            {t('receipt.newSale')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
