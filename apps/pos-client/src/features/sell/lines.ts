/**
 * A cart line as the screens show it. Totals always come from Rust; the
 * unit price here (product + options) is display-only.
 */
import type {
  CartItem,
  ComboRef,
  Menu,
  OpenOrderView,
  OrderLineInput,
  Product,
  Uuid,
} from '@pos/shared';
import { newUuid } from '@pos/shared';

export interface CartLine {
  line_id: Uuid;
  product_id: Uuid;
  name: string;
  unit_price: number;
  sold_by_weight: boolean;
  unit: Product['unit'];
  quantity_milli: number;
  modifier_ids: Uuid[];
  modifier_names: string[];
  course: number | null;
  note: string | null;
  combo: ComboRef | null;
  combo_name: string | null;
  /** Sent to the kitchen (open orders only). */
  fired_at: string | null;
}

export interface CatalogIndex {
  products: ReadonlyMap<string, Product>;
  menu: Menu | undefined;
}

export function modifierInfo(menu: Menu | undefined, ids: readonly Uuid[]) {
  const options = new Map(
    (menu?.modifier_groups ?? []).flatMap((g) => g.modifiers.map((m) => [m.id, m] as const)),
  );
  const chosen = ids.map((id) => options.get(id)).filter((m) => m !== undefined);
  return {
    names: chosen.map((m) => m.name),
    delta: chosen.reduce((sum, m) => sum + m.price_delta, 0),
  };
}

export function newLine(
  product: Product,
  options: {
    quantity_milli?: number;
    modifier_ids?: Uuid[];
    menu?: Menu | undefined;
    course?: number | null;
    note?: string | null;
    combo?: ComboRef | null;
    combo_name?: string | null;
  } = {},
): CartLine {
  const info = modifierInfo(options.menu, options.modifier_ids ?? []);
  return {
    line_id: newUuid(),
    product_id: product.id,
    name: product.name,
    unit_price: product.price + info.delta,
    sold_by_weight: product.sold_by_weight,
    unit: product.unit,
    quantity_milli: options.quantity_milli ?? 1000,
    modifier_ids: options.modifier_ids ?? [],
    modifier_names: info.names,
    course: options.course ?? null,
    note: options.note ?? null,
    combo: options.combo ?? null,
    combo_name: options.combo_name ?? null,
    fired_at: null,
  };
}

/** Same product, same options, not in a combo, not sent: tapping again adds one. */
export function mergeable(a: CartLine, b: CartLine): boolean {
  return (
    a.product_id === b.product_id &&
    !a.sold_by_weight &&
    a.combo === null &&
    b.combo === null &&
    a.fired_at === null &&
    a.course === b.course &&
    a.note === b.note &&
    a.modifier_ids.join() === b.modifier_ids.join()
  );
}

export function addLines(lines: readonly CartLine[], added: readonly CartLine[]): CartLine[] {
  let next = [...lines];
  for (const line of added) {
    const same = next.find((l) => mergeable(l, line));
    next = same
      ? next.map((l) =>
          l === same ? { ...l, quantity_milli: l.quantity_milli + line.quantity_milli } : l,
        )
      : [...next, line];
  }
  return next;
}

export function toCartItems(lines: readonly CartLine[]): CartItem[] {
  return lines.map((l) => ({
    product_id: l.product_id,
    quantity_milli: l.quantity_milli,
    modifier_ids: l.modifier_ids,
    course: l.course,
    note: l.note,
    combo: l.combo,
  }));
}

export function toOrderLines(lines: readonly CartLine[]): OrderLineInput[] {
  return lines.map((l) => ({
    line_id: l.line_id,
    product_id: l.product_id,
    quantity_milli: l.quantity_milli,
    modifier_ids: l.modifier_ids,
    course: l.course,
    note: l.note,
    combo: l.combo,
  }));
}

/** An open order's items as cart lines (names from the catalogue). */
export function orderLines(order: OpenOrderView, index: CatalogIndex): CartLine[] {
  const combos = new Map((index.menu?.combos ?? []).map((c) => [c.id, c.name]));
  return order.items.map((item) => {
    const product = index.products.get(item.product_id);
    const info = modifierInfo(index.menu, item.modifier_ids);
    return {
      line_id: item.line_id,
      product_id: item.product_id,
      name: product?.name ?? '…',
      unit_price: (product?.price ?? 0) + info.delta,
      sold_by_weight: product?.sold_by_weight ?? false,
      unit: product?.unit ?? 'each',
      quantity_milli: item.quantity_milli,
      modifier_ids: item.modifier_ids,
      modifier_names: info.names,
      course: item.course,
      note: item.note,
      combo: item.combo,
      combo_name: item.combo ? (combos.get(item.combo.combo_id) ?? null) : null,
      fired_at: item.fired_at,
    };
  });
}

/** Removing one line of a combo removes the whole combo. */
export function removeLine(lines: readonly CartLine[], lineId: Uuid): CartLine[] {
  const target = lines.find((l) => l.line_id === lineId);
  const instance = target?.combo?.instance;
  return lines.filter((l) => (instance ? l.combo?.instance !== instance : l.line_id !== lineId));
}

export function setQuantity(lines: readonly CartLine[], lineId: Uuid, quantityMilli: number) {
  if (quantityMilli <= 0) return removeLine(lines, lineId);
  return lines.map((l) => (l.line_id === lineId ? { ...l, quantity_milli: quantityMilli } : l));
}
