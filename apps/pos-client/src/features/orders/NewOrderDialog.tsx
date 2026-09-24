import type { DiningTable, OpenOrderInput, Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';

interface Props {
  /** Restaurant: the table tapped on the floor (guests only). */
  table: DiningTable | null;
  /** Cafe: free tables a tab can optionally sit at. */
  freeTables?: readonly DiningTable[];
  title: string;
  confirmLabel: string;
  pending: boolean;
  error: string | null;
  onClose: () => void;
  onOpen: (input: OpenOrderInput) => void;
}

/** Opens a tab (a name, maybe a table) or seats a table (guests). */
export function NewOrderDialog({
  table,
  freeTables = [],
  title,
  confirmLabel,
  pending,
  error,
  onClose,
  onOpen,
}: Props) {
  const { t } = useTranslation();
  const [label, setLabel] = useState('');
  const [tableId, setTableId] = useState<Uuid | null>(table?.id ?? null);
  const [guests, setGuests] = useState(table ? Math.min(table.seats, 2) : 1);
  const needsName = table === null && tableId === null;
  const valid = !needsName || label.trim().length > 0;

  const submit = () => {
    if (!valid || pending) return;
    onOpen({
      table_id: table?.id ?? tableId,
      label: label.trim() || null,
      guests,
      order_type: (table?.id ?? tableId) ? 'dine_in' : 'counter',
    });
  };

  return (
    <Modal open title={title} onClose={onClose}>
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        {table === null && (
          <>
            <label className="field">
              <span>{t('orders.tabName')}</span>
              <input
                autoFocus
                maxLength={40}
                value={label}
                placeholder={t('orders.tabNamePlaceholder')}
                onChange={(e) => {
                  setLabel(e.target.value);
                }}
              />
            </label>
            {freeTables.length > 0 && (
              <label className="field">
                <span>{t('orders.atTable')}</span>
                <select
                  value={tableId ?? ''}
                  onChange={(e) => {
                    setTableId(freeTables.find((ft) => ft.id === e.target.value)?.id ?? null);
                  }}
                >
                  <option value="">{t('orders.noTable')}</option>
                  {freeTables.map((ft) => (
                    <option key={ft.id} value={ft.id}>
                      {ft.label}
                      {ft.area ? ` · ${ft.area}` : ''}
                    </option>
                  ))}
                </select>
              </label>
            )}
          </>
        )}
        <div className="field">
          <span>{t('orders.guests')}</span>
          <div className="stepper stepper--large" dir="ltr">
            <button
              type="button"
              disabled={guests <= 1}
              onClick={() => {
                setGuests(guests - 1);
              }}
              aria-label={t('sell.less')}
            >
              −
            </button>
            <span>{guests}</span>
            <button
              type="button"
              disabled={guests >= 99}
              onClick={() => {
                setGuests(guests + 1);
              }}
              aria-label={t('sell.more')}
            >
              +
            </button>
          </div>
        </div>
        {error && (
          <p role="alert" className="error-text">
            {error}
          </p>
        )}
        <div className="row row--end">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button type="submit" className="button button--primary" disabled={!valid || pending}>
            {confirmLabel}
          </button>
        </div>
      </form>
    </Modal>
  );
}
