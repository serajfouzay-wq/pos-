import type { BackupInfo, BackupSettings } from '@pos/shared';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import {
  useBackupNow,
  useBackupsIn,
  useBackupStatus,
  useRestartApp,
  useRestoreBackup,
  useSaveBackupSettings,
  useSetBackupPassword,
} from '../../ipc/queries';
import { formatDateTime } from '../../lib/dates';
import { useUiStore } from '../../stores/ui';

function size(bytes: number): string {
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  return `${String(Math.max(1, Math.round(bytes / 1024)))} KB`;
}

async function pickFolder(title: string): Promise<string | null> {
  const chosen = await openDialog({ directory: true, multiple: false, title });
  return typeof chosen === 'string' ? chosen : null;
}

/**
 * Restoring a backup: from this till's list, a folder (USB stick) or one
 * file. Also shown on the lock screen when the database cannot be opened.
 */
export function RestorePanel({ backups }: { backups: readonly BackupInfo[] }) {
  const { t } = useTranslation();
  const locale = useUiStore((s) => s.locale);
  const scan = useBackupsIn();
  const restore = useRestoreBackup();
  const restart = useRestartApp();
  const [chosen, setChosen] = useState<BackupInfo | null>(null);
  const [password, setPassword] = useState('');
  const list = scan.data ?? backups;

  return (
    <div className="stack">
      <div className="row row--wrap">
        <button
          type="button"
          className="button"
          onClick={() => {
            void pickFolder(t('backups.pickFolder')).then((dir) => {
              if (dir) scan.mutate(dir);
            });
          }}
        >
          {t('backups.fromFolder')}
        </button>
        {scan.data && (
          <span className="muted small">{t('backups.found', { count: scan.data.length })}</span>
        )}
      </div>
      {list.length === 0 ? (
        <p className="muted">{t('backups.none')}</p>
      ) : (
        <ul className="backup-list">
          {list.slice(0, 50).map((b) => (
            <li key={b.path}>
              <button
                type="button"
                className="backup-list__item"
                aria-pressed={chosen?.path === b.path}
                onClick={() => {
                  setChosen(b);
                  setPassword('');
                  restore.reset();
                }}
              >
                <strong>{formatDateTime(b.created_at, locale)}</strong>
                <span className="muted small">
                  {t(`backups.reasons.${b.reason}`)} · {b.device_name} · {size(b.size_bytes)}
                  {b.portable && ` · 🔑 ${t('backups.portable')}`}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {chosen && (
        <form
          className="stack restore-form"
          onSubmit={(e) => {
            e.preventDefault();
            restore.mutate({ path: chosen.path, password: chosen.portable ? password : null });
          }}
        >
          <p>
            {t('backups.restoreWarning', {
              when: formatDateTime(chosen.created_at, locale),
            })}
          </p>
          {chosen.portable && (
            <label className="field">
              <span>{t('backups.password')}</span>
              <input
                type="password"
                required
                autoComplete="off"
                value={password}
                onChange={(e) => {
                  setPassword(e.target.value);
                }}
              />
            </label>
          )}
          {restore.error && (
            <p role="alert" className="error-text">
              {restore.error.message}
            </p>
          )}
          {restore.isSuccess ? (
            <div className="stack">
              <p className="ok-text">{t('backups.staged')}</p>
              <button
                type="button"
                className="button button--primary"
                onClick={() => {
                  restart.mutate();
                }}
              >
                {t('backups.restartNow')}
              </button>
            </div>
          ) : (
            <button type="submit" className="button button--danger" disabled={restore.isPending}>
              {restore.isPending ? t('common.working') : t('backups.restore')}
            </button>
          )}
        </form>
      )}
    </div>
  );
}

function PasswordDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const set = useSetBackupPassword();
  const [password, setPassword] = useState('');
  const [again, setAgain] = useState('');
  const mismatch = again.length > 0 && password !== again;
  return (
    <Modal open title={t('backups.setPassword')} onClose={onClose}>
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          if (password.length < 6 || mismatch) return;
          set.mutate(password, { onSuccess: onClose });
        }}
      >
        <p className="muted">{t('backups.passwordHelp')}</p>
        <label className="field">
          <span>{t('backups.password')}</span>
          <input
            type="password"
            minLength={6}
            required
            autoComplete="new-password"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
            }}
          />
        </label>
        <label className="field">
          <span>{t('backups.passwordAgain')}</span>
          <input
            type="password"
            required
            autoComplete="new-password"
            value={again}
            onChange={(e) => {
              setAgain(e.target.value);
            }}
          />
        </label>
        {mismatch && <p className="error-text">{t('backups.passwordMismatch')}</p>}
        {set.error && (
          <p role="alert" className="error-text">
            {set.error.message}
          </p>
        )}
        <button
          type="submit"
          className="button button--primary"
          disabled={set.isPending || password.length < 6 || mismatch}
        >
          {t('common.save')}
        </button>
      </form>
    </Modal>
  );
}

/** Owner: this till's backups (automatic, a second copy, restoring). */
export function BackupsScreen() {
  const { t } = useTranslation();
  const locale = useUiStore((s) => s.locale);
  const status = useBackupStatus();
  const backupNow = useBackupNow();
  const save = useSaveBackupSettings();
  const [edited, setEdited] = useState<BackupSettings | null>(null);
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const data = status.data;
  if (!data) return <p className="muted center">{status.error?.message ?? '…'}</p>;
  const draft = edited ?? data.settings;
  const set = (patch: Partial<BackupSettings>) => {
    setEdited({ ...draft, ...patch });
  };

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('backups.title')}</h1>
        <button
          type="button"
          className="button button--primary"
          disabled={backupNow.isPending}
          onClick={() => {
            backupNow.mutate();
          }}
        >
          {backupNow.isPending ? t('common.working') : t('backups.now')}
        </button>
      </header>
      <p className="muted">{t('backups.help')}</p>

      {data.integrity === 'damaged' && (
        <p role="alert" className="banner banner--danger">
          {t('backups.damaged')} {data.integrity_detail}
        </p>
      )}
      {(backupNow.error ?? data.last_error) && (
        <p role="alert" className="error-text">
          {backupNow.error?.message ?? data.last_error}
        </p>
      )}

      <section className="card">
        <div className="backup-facts">
          <div>
            <span className="muted small">{t('backups.last')}</span>
            <strong>
              {data.last_backup_at
                ? formatDateTime(data.last_backup_at, locale)
                : t('backups.never')}
            </strong>
          </div>
          <div>
            <span className="muted small">{t('backups.integrity')}</span>
            <strong className={data.integrity === 'damaged' ? 'tone--bad' : 'tone--good'}>
              {t(`backups.integrityStates.${data.integrity}`)}
            </strong>
          </div>
          <div>
            <span className="muted small">{t('backups.protection')}</span>
            <strong className={data.password_set ? 'tone--good' : 'tone--warn'}>
              {data.password_set ? t('backups.portableOn') : t('backups.portableOff')}
            </strong>
          </div>
          <div>
            <span className="muted small">{t('backups.secondCopy')}</span>
            <strong className={data.extra_dir_ok === false ? 'tone--bad' : undefined}>
              {data.settings.extra_dir
                ? data.extra_dir_ok
                  ? t('backups.copyOk')
                  : t('backups.copyMissing')
                : t('backups.copyNone')}
            </strong>
          </div>
        </div>
        <p className="muted small" dir="ltr">
          {data.dir}
        </p>
        <button
          type="button"
          className="button"
          onClick={() => {
            setPasswordOpen(true);
          }}
        >
          {data.password_set ? t('backups.changePassword') : t('backups.setPassword')}
        </button>
      </section>

      <section className="card">
        <h2>{t('backups.settings')}</h2>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.automatic}
            onChange={(e) => {
              set({ automatic: e.target.checked });
            }}
          />
          {t('backups.automatic')}
        </label>
        <div className="form-grid">
          <label className="field">
            <span>{t('backups.every')}</span>
            <select
              value={draft.interval_hours}
              onChange={(e) => {
                set({ interval_hours: Number(e.target.value) });
              }}
            >
              {[1, 3, 6, 12, 24].map((h) => (
                <option key={h} value={h}>
                  {t('backups.hours', { count: h })}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>{t('backups.keep')}</span>
            <select
              value={draft.keep}
              onChange={(e) => {
                set({ keep: Number(e.target.value) });
              }}
            >
              {[10, 30, 60, 120].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="field">
          <span>{t('backups.secondCopy')}</span>
          <div className="row row--wrap">
            <code className="grow" dir="ltr">
              {draft.extra_dir ?? t('backups.copyNone')}
            </code>
            <button
              type="button"
              className="button"
              onClick={() => {
                void pickFolder(t('backups.pickFolder')).then((dir) => {
                  if (dir) set({ extra_dir: dir });
                });
              }}
            >
              {t('backups.chooseFolder')}
            </button>
            {draft.extra_dir && (
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  set({ extra_dir: null });
                }}
              >
                {t('sell.remove')}
              </button>
            )}
          </div>
          <span className="muted small">{t('backups.secondCopyHelp')}</span>
        </div>
        {save.error && (
          <p role="alert" className="error-text">
            {save.error.message}
          </p>
        )}
        <button
          type="button"
          className="button button--primary"
          disabled={save.isPending || !edited}
          onClick={() => {
            save.mutate(draft, {
              onSuccess: () => {
                setEdited(null);
              },
            });
          }}
        >
          {t('common.save')}
        </button>
      </section>

      <section className="card">
        <div className="row">
          <h2 className="grow">{t('backups.history')}</h2>
          <button
            type="button"
            className="button button--danger"
            onClick={() => {
              setRestoring(!restoring);
            }}
          >
            {t('backups.restore')}…
          </button>
        </div>
        {restoring ? (
          <RestorePanel backups={data.backups} />
        ) : data.backups.length === 0 ? (
          <p className="muted">{t('backups.none')}</p>
        ) : (
          <table className="table">
            <tbody>
              {data.backups.slice(0, 15).map((b) => (
                <tr key={b.path}>
                  <td>{formatDateTime(b.created_at, locale)}</td>
                  <td>{t(`backups.reasons.${b.reason}`)}</td>
                  <td className="num">{size(b.size_bytes)}</td>
                  <td>{b.portable ? '🔑' : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      {passwordOpen && (
        <PasswordDialog
          onClose={() => {
            setPasswordOpen(false);
          }}
        />
      )}
    </div>
  );
}
