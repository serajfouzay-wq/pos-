import { FLOOR_COLUMNS, FLOOR_ROWS, type DiningTable, type OpenOrderView } from '@pos/shared';
import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { useMoney } from '../../lib/money';

/** Cells a table covers on the 24 × 16 floor grid. */
export function tableSpan(table: Pick<DiningTable, 'shape' | 'grid_x' | 'grid_y'>) {
  const size = table.shape === 'square' ? 3 : 2;
  return {
    columns: Math.min(size, FLOOR_COLUMNS - table.grid_x),
    rows: Math.min(size, FLOOR_ROWS - table.grid_y),
  };
}

export function tableStyle(table: Pick<DiningTable, 'shape' | 'grid_x' | 'grid_y'>) {
  const span = tableSpan(table);
  return {
    gridColumn: `${table.grid_x + 1} / span ${span.columns}`,
    gridRow: `${table.grid_y + 1} / span ${span.rows}`,
  };
}

interface Props {
  tables: readonly DiningTable[];
  orders: readonly OpenOrderView[];
  onTable: (table: DiningTable, order: OpenOrderView | undefined) => void;
}

/** The floor: free tables, occupied ones with their total, unsent items flagged. */
export function TableMap({ tables, orders, onTable }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const byTable = new Map(
    orders.filter((o) => o.table_id !== null).map((o) => [o.table_id, o] as const),
  );
  const areas = [...new Set(tables.map((table) => table.area).filter((a) => a.length > 0))];

  if (tables.length === 0) return <p className="muted floor__empty">{t('orders.noTables')}</p>;

  return (
    <div className="floor">
      {areas.length > 1 && <p className="muted small floor__areas">{areas.join(' · ')}</p>}
      <div
        className="floor__grid"
        style={{
          gridTemplateColumns: `repeat(${FLOOR_COLUMNS}, 1fr)`,
          gridTemplateRows: `repeat(${FLOOR_ROWS}, 1fr)`,
        }}
      >
        {tables.map((table) => {
          const order = byTable.get(table.id);
          const state = !order ? 'free' : order.unfired > 0 ? 'unsent' : 'occupied';
          return (
            <motion.button
              key={table.id}
              type="button"
              className={`table-tile table-tile--${table.shape} table-tile--${state}`}
              style={tableStyle(table)}
              whileTap={{ scale: 0.96 }}
              onClick={() => {
                onTable(table, order);
              }}
              aria-label={`${table.label} · ${t(`orders.state.${state}`)}`}
            >
              <span className="table-tile__label">{table.label}</span>
              {order ? (
                <>
                  <span className="table-tile__total">
                    {order.total === null ? '—' : format(order.total)}
                  </span>
                  <span className="small">
                    {t('orders.guestCount', { count: order.guests })}
                    {order.unfired > 0 && ` · ${t('orders.unsent', { count: order.unfired })}`}
                  </span>
                </>
              ) : (
                <span className="small muted">{t('orders.seats', { count: table.seats })}</span>
              )}
            </motion.button>
          );
        })}
      </div>
      <ul className="floor__legend small">
        {(['free', 'occupied', 'unsent'] as const).map((state) => (
          <li key={state}>
            <span className={`legend-dot table-tile--${state}`} /> {t(`orders.state.${state}`)}
          </li>
        ))}
      </ul>
    </div>
  );
}
