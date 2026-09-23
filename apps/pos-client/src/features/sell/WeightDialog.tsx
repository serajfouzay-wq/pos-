import type { Product } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { Numpad } from '../../components/Numpad';
import { formatQuantity } from '../../lib/money';

interface Props {
  product: Pick<Product, 'name' | 'unit'> | null;
  initialMilli?: number;
  onConfirm: (quantityMilli: number) => void;
  onClose: () => void;
}

/** Weighed goods: enter the weight in thousandths (1 · 2 · 5 · 0 → 1.25 kg). */
export function WeightDialog({ product, initialMilli = 0, onConfirm, onClose }: Props) {
  const { t } = useTranslation();
  const [milli, setMilli] = useState(initialMilli);
  const digits = milli === 0 ? '' : String(milli);
  const confirm = () => {
    if (milli > 0) {
      onConfirm(milli);
      setMilli(0);
    }
  };
  return (
    <Modal open={product !== null} title={product?.name ?? ''} onClose={onClose}>
      <div className="stack">
        <output className="amountpad__display" dir="ltr">
          {formatQuantity(milli)} {product?.unit}
        </output>
        <Numpad
          onDigit={(d) => {
            if (digits.length < 9) setMilli(Number(digits + d));
          }}
          onBackspace={() => {
            setMilli(Number(digits.slice(0, -1) || '0'));
          }}
          onClear={() => {
            setMilli(0);
          }}
          onEnter={confirm}
        />
        <button
          type="button"
          className="button button--primary button--block"
          disabled={milli <= 0}
          onClick={confirm}
        >
          {t('sell.addWeight')}
        </button>
      </div>
    </Modal>
  );
}
