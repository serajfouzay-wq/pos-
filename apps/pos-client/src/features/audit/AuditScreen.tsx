import type { AuditEntryView, Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuditLog, useUsers } from '../../ipc/queries';
import { formatDateTime, presetRange, type RangePreset } from '../../lib/dates';
import { useUiStore } from '../../stores/ui';

type Range = RangePreset | 'all';
const RANGES: readonly Range[] = ['today', 'last7', 'last30', 'all'];
const PAGE = 50;

/** `sale.refund` → `sale.` groups for the filter. */
function groups(actions: readonly string[]): string[] {
  return [...new Set(actions.map((a) => `${a.split('.')[0] ?? a}.`))];
}

/** The receipt number an entry's details mention, if any. */
function receiptOf(detail: unknown): string | null {
  if (detail === null || typeof detail !== 'object' || Array.isArray(detail)) return null;
  const value = (detail as Record<string, unknown>)['receipt_number'];
  return typeof value === 'string' ? value : null;
}

function Json({ label, value }: { label: string; value: unknown }) {
  if (value === null || value === undefined) return null;
  return (
    <div>
      <span className="muted small">{label}</span>
      <pre className="audit__json" dir="ltr">
        {JSON.stringify(value, null, 2)}
      </pre>
    </div>
  );
}

function Entry({ entry }: { entry: AuditEntryView }) {
  const { t } = useTranslation();
  const locale = useUiStore((s) => s.locale);
  const [open, setOpen] = useState(false);
  const detail = entry.after ?? entry.before;
  return (
    <>
      <tr
        className="table__link"
        tabIndex={0}
        onClick={() => {
          setOpen(!open);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') setOpen(!open);
        }}
      >
        <td>{formatDateTime(entry.occurred_at, locale)}</td>
        <td>
          {entry.user_name ?? '—'} <span className="muted small">· {t(`roles.${entry.role}`)}</span>
        </td>
        <td>
          <code>{entry.action}</code>
        </td>
        <td className="muted small">
          {entry.entity_type}
          {receiptOf(detail) && ` · ${receiptOf(detail) ?? ''}`}
        </td>
      </tr>
      {open && (
        <tr className="audit__detail">
          <td colSpan={4}>
            <div className="audit__diff">
              <Json label={t('audit.before')} value={entry.before} />
              <Json label={t('audit.after')} value={entry.after} />
              {entry.before === null && entry.after === null && (
                <span className="muted small">{t('audit.noDetail')}</span>
              )}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

/** Owner: who did what, when — every privileged action on every till. */
export function AuditScreen() {
  const { t } = useTranslation();
  const users = useUsers();
  const [range, setRange] = useState<Range>('today');
  const [action, setAction] = useState<string | null>(null);
  const [userId, setUserId] = useState<Uuid | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const [anchor] = useState(() => new Date());
  const bounds = range === 'all' ? { from: null, to: null } : presetRange(range, anchor);
  const log = useAuditLog({ ...bounds, action, user_id: userId, limit });
  const page = log.data;
  const actions = page?.actions ?? [];

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('audit.title')}</h1>
        <div className="row row--wrap">
          {RANGES.map((r) => (
            <button
              key={r}
              type="button"
              className="chip"
              aria-pressed={range === r}
              onClick={() => {
                setRange(r);
                setLimit(PAGE);
              }}
            >
              {t(`ranges.${r}`)}
            </button>
          ))}
          <select
            aria-label={t('audit.action')}
            value={action ?? ''}
            onChange={(e) => {
              setAction(e.target.value || null);
              setLimit(PAGE);
            }}
          >
            <option value="">{t('audit.allActions')}</option>
            {groups(actions).map((g) => (
              <option key={g} value={g}>
                {t('audit.group', { group: g.slice(0, -1) })}
              </option>
            ))}
            {actions.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </select>
          <select
            aria-label={t('audit.user')}
            value={userId ?? ''}
            onChange={(e) => {
              setUserId(users.data?.find((u) => u.id === e.target.value)?.id ?? null);
              setLimit(PAGE);
            }}
          >
            <option value="">{t('audit.allUsers')}</option>
            {users.data?.map((u) => (
              <option key={u.id} value={u.id}>
                {u.display_name}
              </option>
            ))}
          </select>
        </div>
      </header>
      {log.error && (
        <p role="alert" className="error-text">
          {log.error.message}
        </p>
      )}
      <p className="muted small">{t('audit.count', { count: page?.total ?? 0 })}</p>
      <table className="table">
        <thead>
          <tr>
            <th>{t('history.time')}</th>
            <th>{t('audit.user')}</th>
            <th>{t('audit.action')}</th>
            <th>{t('audit.entity')}</th>
          </tr>
        </thead>
        <tbody>
          {page?.entries.length === 0 && (
            <tr>
              <td colSpan={4} className="muted">
                {t('audit.empty')}
              </td>
            </tr>
          )}
          {page?.entries.map((entry) => (
            <Entry key={entry.id} entry={entry} />
          ))}
        </tbody>
      </table>
      {page && page.entries.length < page.total && (
        <button
          type="button"
          className="button"
          onClick={() => {
            setLimit(limit + PAGE);
          }}
        >
          {t('common.more')}
        </button>
      )}
    </div>
  );
}
