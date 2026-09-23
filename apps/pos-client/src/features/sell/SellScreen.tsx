import type { Product, SaleReceipt, Session } from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipc } from '../../ipc';
import { useQuote } from '../../ipc/queries';
import { useBarcodeScanner } from '../../lib/useBarcodeScanner';
import { CartPanel } from './CartPanel';
import { toCartItems, useCart } from './cartStore';
import { PaymentDialog } from './PaymentDialog';
import { ProductGrid } from './ProductGrid';
import { ReceiptDialog } from './ReceiptDialog';
import { WeightDialog } from './WeightDialog';

/** A weighed item being entered: a fresh pick, or an existing cart line. */
interface Weighing {
  productId: string;
  name: string;
  unit: Product['unit'];
  initialMilli: number;
  product: Product | null;
}

export function SellScreen({ session }: { session: Session }) {
  const { t } = useTranslation();
  const { lines, add, clear, setQuantity } = useCart();
  const [weighing, setWeighing] = useState<Weighing | null>(null);
  const [paying, setPaying] = useState(false);
  const [receipt, setReceipt] = useState<SaleReceipt | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const request = useMemo(
    () => (lines.length ? { items: toCartItems(lines), discount_rule_ids: [] } : null),
    [lines],
  );
  const quote = useQuote(request);

  useEffect(() => {
    if (!toast) return undefined;
    const id = setTimeout(() => {
      setToast(null);
    }, 2500);
    return () => {
      clearTimeout(id);
    };
  }, [toast]);

  const pick = (product: Product) => {
    if (product.sold_by_weight) {
      setWeighing({
        productId: product.id,
        name: product.name,
        unit: product.unit,
        initialMilli: 0,
        product,
      });
    } else {
      add(product);
    }
  };

  useBarcodeScanner(
    (code) => {
      void ipc
        .call('get_products', { filter: { barcode: code, limit: 1 } })
        .then(([product]) => {
          if (product) pick(product);
          else setToast(t('sell.unknownBarcode', { code }));
        })
        .catch((e: unknown) => {
          setToast(e instanceof Error ? e.message : String(e));
        });
    },
    !paying && !receipt && !weighing,
  );

  return (
    <div className="sell">
      <ProductGrid onPick={pick} />
      <CartPanel
        quote={quote.data}
        quoteError={quote.error?.message}
        quoting={quote.isFetching}
        onPay={() => {
          setPaying(true);
        }}
        onEditWeight={(id) => {
          const line = lines.find((l) => l.product_id === id);
          if (line) {
            setWeighing({
              productId: id,
              name: line.name,
              unit: line.unit,
              initialMilli: line.quantity_milli,
              product: null,
            });
          }
        }}
      />
      <WeightDialog
        key={weighing?.productId ?? 'none'}
        product={weighing ? { name: weighing.name, unit: weighing.unit } : null}
        initialMilli={weighing?.initialMilli ?? 0}
        onClose={() => {
          setWeighing(null);
        }}
        onConfirm={(milli) => {
          if (weighing?.product) add(weighing.product, milli);
          else if (weighing) setQuantity(weighing.productId, milli);
          setWeighing(null);
        }}
      />
      {quote.data && (
        <PaymentDialog
          key={String(paying)}
          open={paying}
          total={quote.data.total}
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
      <AnimatePresence>
        {toast && (
          <motion.div
            className="toast"
            role="status"
            initial={{ y: 40, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            {toast}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
