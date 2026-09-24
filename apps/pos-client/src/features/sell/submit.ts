import type { OrderType, Uuid } from '@pos/shared';
import { useCreateTransaction, usePayOrder } from '../../ipc/queries';
import { toCartItems, type CartLine } from './lines';
import type { PaymentSubmit } from './PaymentDialog';

/** Quick sale: `create_transaction` from the local cart. */
export function useCartSubmit(
  lines: readonly CartLine[],
  orderType: OrderType = 'counter',
): PaymentSubmit {
  const create = useCreateTransaction();
  return {
    run: (payments, key, { onSuccess }) => {
      create.mutate(
        {
          idempotency_key: key,
          customer_id: null,
          order_type: orderType,
          table_label: null,
          items: toCartItems(lines),
          discount_rule_ids: [],
          loyalty_points_to_redeem: 0,
          payments,
          notes: null,
        },
        { onSuccess },
      );
    },
    pending: create.isPending,
    error: create.error,
    reset: create.reset,
  };
}

/** Open order: `pay_open_order` for the chosen lines (null = all). */
export function useOrderSubmit(orderId: Uuid | null, lineIds: Uuid[] | null): PaymentSubmit {
  const pay = usePayOrder();
  return {
    run: (payments, key, { onSuccess }) => {
      if (!orderId) return;
      pay.mutate(
        {
          order_id: orderId,
          idempotency_key: key,
          line_ids: lineIds,
          discount_rule_ids: [],
          payments,
        },
        {
          onSuccess: (paid) => {
            onSuccess(paid.sale);
          },
        },
      );
    },
    pending: pay.isPending,
    error: pay.error,
    reset: pay.reset,
  };
}
