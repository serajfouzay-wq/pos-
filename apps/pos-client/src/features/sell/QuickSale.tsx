import type { OrderType, SaleReceipt, Session } from '@pos/shared';
import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useToast } from '../../components/Toast';
import { useQuote } from '../../ipc/queries';
import { CartPanel } from './CartPanel';
import { useCart } from './cartStore';
import { toCartItems, type CartLine } from './lines';
import { PaymentDialog } from './PaymentDialog';
import { ProductGrid } from './ProductGrid';
import { ReceiptDialog } from './ReceiptDialog';
import { useCartSubmit } from './submit';
import { useBarcodeLookup } from './useBarcodeLookup';
import { useCatalogIndex } from './useCatalogIndex';
import { useProductPicker } from './useProductPicker';
import { useNoteEdit } from './useNoteEdit';
import { useWeightEdit } from './useWeightEdit';

interface Props {
  session: Session;
  orderType: OrderType;
  /** Retail: open on the quick-keys grid; Enter in search is a barcode lookup. */
  retail?: boolean;
  /** Bar above the screen (restaurant takeaway: back to the floor). */
  header?: ReactNode;
  /** Cafe: park the cart as a named tab instead of paying now. */
  onHold?: (lines: CartLine[]) => void;
}

/** Pay-now selling from the local cart: retail counter, cafe counter, takeaway. */
export function QuickSale({ session, orderType, retail = false, header, onHold }: Props) {
  const { t } = useTranslation();
  const { lines, add, clear, setQuantity, remove, replace } = useCart();
  const index = useCatalogIndex();
  const toast = useToast();
  const [paying, setPaying] = useState(false);
  const [receipt, setReceipt] = useState<SaleReceipt | null>(null);

  const request = useMemo(
    () => (lines.length ? { items: toCartItems(lines), discount_rule_ids: [] } : null),
    [lines],
  );
  const quote = useQuote(request);
  const submit = useCartSubmit(lines, orderType);
  const picker = useProductPicker({
    menu: index.menu,
    products: index.products,
    course: null,
    allowNote: !retail,
    onAdd: add,
  });
  const weight = useWeightEdit(setQuantity);
  const note = useNoteEdit((lineId, text) => {
    replace(lines.map((l) => (l.line_id === lineId ? { ...l, note: text } : l)));
  });
  const lookup = useBarcodeLookup(
    picker.pick,
    toast.show,
    !paying && !receipt && !picker.busy && !weight.busy && !note.busy,
  );

  return (
    <div className={header ? 'sell sell--with-bar' : 'sell'}>
      {header}
      <ProductGrid
        onPick={picker.pick}
        combos={retail ? [] : (index.menu?.combos ?? [])}
        onPickCombo={picker.pickCombo}
        quickKeys={retail}
        {...(retail ? { onSubmitSearch: lookup } : {})}
      />
      <CartPanel
        title={t('sell.cart')}
        lines={lines}
        quote={quote.data}
        quoteError={quote.error?.message}
        quoting={quote.isFetching}
        onSetQuantity={setQuantity}
        onRemove={remove}
        onEditWeight={weight.edit}
        onEditNote={retail ? undefined : note.edit}
        onClear={clear}
        actions={
          onHold && lines.length > 0 ? (
            <button
              type="button"
              className="button button--block"
              onClick={() => {
                onHold(lines);
              }}
            >
              {t('orders.holdAsTab')}
            </button>
          ) : undefined
        }
        onPay={() => {
          setPaying(true);
        }}
      />
      {picker.dialogs}
      {weight.dialog}
      {note.dialog}
      {quote.data && (
        <PaymentDialog
          key={String(paying)}
          open={paying}
          total={quote.data.total}
          submit={submit}
          onClose={() => {
            setPaying(false);
          }}
          onComplete={(r) => {
            setPaying(false);
            setReceipt(r);
          }}
        />
      )}
      <ReceiptDialog
        receipt={receipt}
        session={session}
        onNewSale={() => {
          clear();
          setReceipt(null);
        }}
      />
      {toast.node}
    </div>
  );
}
