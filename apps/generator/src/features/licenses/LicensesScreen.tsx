import { MIN_SIGNING_PASSPHRASE_LENGTH, type SigningKeyStatus } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../components/CopyButton';
import { useSigningKey, useSigningKeyAction } from '../../ipc/queries';
import { IssueLicenseForm } from './IssueLicenseForm';

function KeyCard({ status }: { status: SigningKeyStatus }) {
  const { t } = useTranslation();
  const create = useSigningKeyAction('create_license_key');
  const unlock = useSigningKeyAction('unlock_license_key');
  const lock = useSigningKeyAction('lock_license_key');
  const [passphrase, setPassphrase] = useState('');
  const [confirm, setConfirm] = useState('');

  const action = status.state === 'absent' ? create : unlock;
  const tooShort = passphrase.length < MIN_SIGNING_PASSPHRASE_LENGTH;
  const mismatch = status.state === 'absent' && passphrase !== confirm;

  return (
    <section className="card">
      <h2>{t('licenses.key.title')}</h2>
      <p className="muted">{t(`licenses.key.state.${status.state}`)}</p>

      {status.state !== 'unlocked' && (
        <form
          className="form"
          onSubmit={(e) => {
            e.preventDefault();
            action.mutate(passphrase, {
              onSuccess: () => {
                setPassphrase('');
                setConfirm('');
              },
            });
          }}
        >
          {status.state === 'absent' && (
            <p className="warning">{t('licenses.key.backupWarning')}</p>
          )}
          <label>
            {t('licenses.key.passphrase')}
            <input
              type="password"
              autoComplete="new-password"
              value={passphrase}
              onChange={(e) => {
                setPassphrase(e.target.value);
              }}
            />
          </label>
          {status.state === 'absent' && (
            <label>
              {t('licenses.key.confirm')}
              <input
                type="password"
                autoComplete="new-password"
                value={confirm}
                onChange={(e) => {
                  setConfirm(e.target.value);
                }}
              />
            </label>
          )}
          {action.error && (
            <p role="alert" className="error">
              {action.error.message}
            </p>
          )}
          <button
            type="submit"
            className="button button--primary"
            disabled={tooShort || mismatch || action.isPending}
          >
            {action.isPending
              ? t('common.working')
              : status.state === 'absent'
                ? t('licenses.key.create')
                : t('licenses.key.unlock')}
          </button>
          {tooShort && (
            <span className="muted">
              {t('licenses.key.minLength', { count: MIN_SIGNING_PASSPHRASE_LENGTH })}
            </span>
          )}
        </form>
      )}

      {status.state !== 'absent' && status.public_key_pem && (
        <div className="form">
          <p className="muted">
            {t('licenses.key.keyId')} <code>{status.key_id}</code>
          </p>
          <p className="muted">{t('licenses.key.publicKeyHelp')}</p>
          <textarea className="mono" readOnly rows={6} value={status.public_key_pem} />
          <div className="row">
            <CopyButton text={status.public_key_pem} />
            {status.state === 'unlocked' && (
              <button
                type="button"
                className="button"
                onClick={() => {
                  lock.mutate('');
                }}
              >
                {t('licenses.key.lock')}
              </button>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

function IssueCard({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation();
  return (
    <section className="card" aria-disabled={!enabled}>
      <h2>{t('licenses.issue.title')}</h2>
      {!enabled && <p className="muted">{t('licenses.issue.unlockFirst')}</p>}
      <p className="muted">{t('licenses.issue.anyClientHelp')}</p>
      <IssueLicenseForm enabled={enabled} />
    </section>
  );
}

export function LicensesScreen() {
  const key = useSigningKey();
  return (
    <div className="stack">
      {key.data && <KeyCard status={key.data} />}
      {key.error && (
        <p role="alert" className="error">
          {key.error.message}
        </p>
      )}
      <IssueCard enabled={key.data?.state === 'unlocked'} />
    </div>
  );
}
