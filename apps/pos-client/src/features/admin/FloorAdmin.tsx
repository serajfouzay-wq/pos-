import {
  FLOOR_COLUMNS,
  FLOOR_ROWS,
  TABLE_SHAPES,
  type DiningTable,
  type DiningTableInput,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useDeleteTable, useMenu, useSaveTable } from '../../ipc/queries';
import { tableStyle } from '../orders/TableMap';

const clamp = (n: number, max: number) => Math.min(Math.max(0, n), max);

function inputFrom(table: DiningTable): DiningTableInput {
  return {
    id: table.id,
    label: table.label,
    area: table.area,
    seats: table.seats,
    shape: table.shape,
    grid_x: table.grid_x,
    grid_y: table.grid_y,
    is_active: table.is_active,
  };
}

/** Back office: place tables on the 24 × 16 floor grid. */
export function FloorAdmin() {
  const { t } = useTranslation();
  const menu = useMenu(true);
  const save = useSaveTable();
  const remove = useDeleteTable();
  const [draft, setDraft] = useState<DiningTableInput | null>(null);
  const tables = menu.data?.dining_tables ?? [];

  const cells = Array.from({ length: FLOOR_COLUMNS * FLOOR_ROWS }, (_, i) => ({
    x: i % FLOOR_COLUMNS,
    y: Math.trunc(i / FLOOR_COLUMNS),
  }));
  const set = <K extends keyof DiningTableInput>(key: K, value: DiningTableInput[K]) => {
    setDraft((d) => (d ? { ...d, [key]: value } : d));
  };
  const nudge = (dx: number, dy: number) => {
    if (!draft) return;
    setDraft({
      ...draft,
      grid_x: clamp(draft.grid_x + dx, FLOOR_COLUMNS - 1),
      grid_y: clamp(draft.grid_y + dy, FLOOR_ROWS - 1),
    });
  };

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('admin.floor.title')}</h1>
      </header>
      <p className="muted">{t('admin.floor.help')}</p>
      <div className="floor-editor">
        <div
          className="floor__grid floor__grid--editor"
          style={{
            gridTemplateColumns: `repeat(${FLOOR_COLUMNS}, 1fr)`,
            gridTemplateRows: `repeat(${FLOOR_ROWS}, 1fr)`,
          }}
        >
          {cells.map((c) => (
            <button
              key={`${c.x}-${c.y}`}
              type="button"
              className="floor__cell"
              style={{ gridColumn: c.x + 1, gridRow: c.y + 1 }}
              aria-label={t('admin.floor.addAt', { x: c.x + 1, y: c.y + 1 })}
              onClick={() => {
                setDraft({
                  id: null,
                  label: `T${String(tables.length + 1)}`,
                  area: '',
                  seats: 4,
                  shape: 'square',
                  grid_x: c.x,
                  grid_y: c.y,
                  is_active: true,
                });
              }}
            />
          ))}
          {tables
            .filter((table) => table.id !== draft?.id)
            .map((table) => (
              <button
                key={table.id}
                type="button"
                className={`table-tile table-tile--${table.shape} table-tile--free${table.is_active ? '' : ' inactive'}`}
                style={tableStyle(table)}
                onClick={() => {
                  setDraft(inputFrom(table));
                }}
              >
                <span className="table-tile__label">{table.label}</span>
                <span className="small muted">{t('orders.seats', { count: table.seats })}</span>
              </button>
            ))}
          {draft && (
            <div
              className={`table-tile table-tile--${draft.shape} table-tile--editing`}
              style={tableStyle(draft)}
            >
              <span className="table-tile__label">{draft.label || '?'}</span>
            </div>
          )}
        </div>
        {draft && (
          <form
            className="card stack floor-editor__form"
            onSubmit={(e) => {
              e.preventDefault();
              save.mutate(draft, {
                onSuccess: () => {
                  setDraft(null);
                },
              });
            }}
          >
            <label className="field">
              {t('admin.floor.label')}
              <input
                required
                maxLength={16}
                value={draft.label}
                onChange={(e) => {
                  set('label', e.target.value);
                }}
              />
            </label>
            <label className="field">
              {t('admin.floor.area')}
              <input
                maxLength={40}
                value={draft.area}
                onChange={(e) => {
                  set('area', e.target.value);
                }}
              />
            </label>
            <label className="field">
              {t('admin.floor.seats')}
              <input
                type="number"
                min={1}
                max={50}
                value={draft.seats}
                onChange={(e) => {
                  set('seats', clamp(Math.trunc(Number(e.target.value)), 50) || 1);
                }}
              />
            </label>
            <label className="field">
              {t('admin.floor.shape')}
              <select
                value={draft.shape}
                onChange={(e) => {
                  const shape = TABLE_SHAPES.find((s) => s === e.target.value);
                  if (shape) set('shape', shape);
                }}
              >
                {TABLE_SHAPES.map((s) => (
                  <option key={s} value={s}>
                    {t(`admin.floor.shapes.${s}`)}
                  </option>
                ))}
              </select>
            </label>
            <div className="field">
              <span>{t('admin.floor.position', { x: draft.grid_x + 1, y: draft.grid_y + 1 })}</span>
              <div className="row" dir="ltr">
                <button
                  type="button"
                  className="chip"
                  onClick={() => {
                    nudge(-1, 0);
                  }}
                  aria-label="←"
                >
                  ←
                </button>
                <button
                  type="button"
                  className="chip"
                  onClick={() => {
                    nudge(0, -1);
                  }}
                  aria-label="↑"
                >
                  ↑
                </button>
                <button
                  type="button"
                  className="chip"
                  onClick={() => {
                    nudge(0, 1);
                  }}
                  aria-label="↓"
                >
                  ↓
                </button>
                <button
                  type="button"
                  className="chip"
                  onClick={() => {
                    nudge(1, 0);
                  }}
                  aria-label="→"
                >
                  →
                </button>
              </div>
            </div>
            <label className="check">
              <input
                type="checkbox"
                checked={draft.is_active}
                onChange={(e) => {
                  set('is_active', e.target.checked);
                }}
              />
              {t('admin.menu.active')}
            </label>
            {(save.error ?? remove.error) && (
              <p role="alert" className="error-text">
                {(save.error ?? remove.error)?.message}
              </p>
            )}
            <div className="row">
              <button type="submit" className="button button--primary" disabled={save.isPending}>
                {t('common.save')}
              </button>
              <button
                type="button"
                className="button"
                onClick={() => {
                  setDraft(null);
                }}
              >
                {t('common.cancel')}
              </button>
              {draft.id && (
                <button
                  type="button"
                  className="button button--danger"
                  disabled={remove.isPending}
                  onClick={() => {
                    if (draft.id)
                      remove.mutate(draft.id, {
                        onSuccess: () => {
                          setDraft(null);
                        },
                      });
                  }}
                >
                  {t('admin.menu.delete')}
                </button>
              )}
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
