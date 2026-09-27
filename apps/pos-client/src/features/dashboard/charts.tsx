/**
 * Small, dependency-free charts. Values are minor units from Rust; the
 * charts only scale them for drawing.
 */
import type { ReactNode } from 'react';

export interface Bar {
  key: string;
  label: string;
  value: number;
  /** Tooltip / screen-reader text. */
  title: string;
}

/** Vertical bars on a zero baseline (refunds can make a bucket negative). */
export function BarChart({
  bars,
  height = 150,
  labelEvery = 1,
}: {
  bars: readonly Bar[];
  height?: number;
  labelEvery?: number;
}) {
  const max = Math.max(0, ...bars.map((b) => b.value));
  const min = Math.min(0, ...bars.map((b) => b.value));
  const span = max - min || 1;
  const slot = 24;
  const width = bars.length * slot;
  const zero = (max / span) * height;
  const columns = { gridTemplateColumns: `repeat(${String(bars.length)}, 1fr)` };
  return (
    <div className="bar-chart" dir="ltr">
      {/* Bars stretch to the card; labels stay HTML so text is never distorted. */}
      <svg
        className="bar-chart__plot"
        viewBox={`0 0 ${String(width)} ${String(height)}`}
        preserveAspectRatio="none"
        role="img"
      >
        <line x1={0} x2={width} y1={zero} y2={zero} className="bar-chart__axis" />
        {bars.map((b, i) => {
          const h = (Math.abs(b.value) / span) * height;
          return (
            <rect
              key={b.key}
              x={i * slot + 4}
              y={b.value >= 0 ? zero - h : zero}
              width={slot - 8}
              height={b.value === 0 ? 0 : Math.max(h, 1)}
              className={b.value < 0 ? 'bar-chart__bar bar-chart__bar--negative' : 'bar-chart__bar'}
            >
              <title>{b.title}</title>
            </rect>
          );
        })}
      </svg>
      <div className="bar-chart__labels" style={columns}>
        {bars.map((b, i) => (
          <span key={b.key}>{i % labelEvery === 0 ? b.label : ''}</span>
        ))}
      </div>
    </div>
  );
}

export interface Share {
  key: string;
  label: ReactNode;
  value: number;
  display: string;
  sub?: string;
}

/** Horizontal bars as a list (mirrors in RTL). */
export function ShareList({ items, empty }: { items: readonly Share[]; empty: string }) {
  const max = Math.max(1, ...items.map((i) => Math.abs(i.value)));
  if (items.length === 0) return <p className="muted">{empty}</p>;
  return (
    <ul className="share-list">
      {items.map((item) => (
        <li key={item.key}>
          <div className="share-list__text">
            <span className="share-list__label">{item.label}</span>
            <span className="share-list__value">
              {item.display}
              {item.sub && <span className="muted small"> · {item.sub}</span>}
            </span>
          </div>
          <div className="share-list__track">
            <div
              className={
                item.value < 0 ? 'share-list__fill share-list__fill--negative' : 'share-list__fill'
              }
              style={{ inlineSize: `${String((Math.abs(item.value) / max) * 100)}%` }}
            />
          </div>
        </li>
      ))}
    </ul>
  );
}

/** A headline figure with the change against the previous period. */
export function Kpi({
  label,
  value,
  delta,
  hint,
}: {
  label: string;
  value: string;
  /** Fraction (0.12 = +12 %); null when there is nothing to compare. */
  delta?: number | null;
  hint?: string;
}) {
  return (
    <div className="kpi card">
      <span className="kpi__label">{label}</span>
      <strong className="kpi__value">{value}</strong>
      {delta !== undefined && delta !== null && (
        <span className={delta >= 0 ? 'kpi__delta tone--good' : 'kpi__delta tone--bad'} dir="ltr">
          {delta >= 0 ? '▲' : '▼'} {Math.abs(Math.round(delta * 100))}%
        </span>
      )}
      {hint && <span className="muted small">{hint}</span>}
    </div>
  );
}

/** Relative change, for display only. */
export function change(current: number, previous: number): number | null {
  if (previous === 0) return null;
  return (current - previous) / Math.abs(previous);
}
