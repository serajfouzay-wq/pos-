import {
  BUSINESS_TYPES,
  MIN_SIGNING_PASSPHRASE_LENGTH,
  type BusinessType,
  type SigningKeyStatus,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useDecodeActivation,
  useIssueLicense,
  useSigningKey,
  useSigningKeyAction,
} from '../../ipc/queries';

function CopyButton({ text }: { text: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="button"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
        });
      }}
    >
      {copied ? t('licenses.copied') : t('licenses.copy')}
    </button>
  );
}

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
              ? t('licenses.working')
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
  const decode = useDecodeActivation();
  const issue = useIssueLicense();
  const [code, setCode] = useState('');
  const [slug, setSlug] = useState('');
  const [businessType, setBusinessType] = useState<BusinessType>('retail');
  const [maxDevices, setMaxDevices] = useState(1);
  const [expiry, setExpiry] = useState('');

  return (
    <section className="card" aria-disabled={!enabled}>
      <h2>{t('licenses.issue.title')}</h2>
      {!enabled && <p className="muted">{t('licenses.issue.unlockFirst')}</p>}
      <form
        className="form"
        onSubmit={(e) => {
          e.preventDefault();
          issue.mutate({
            activation_code: code,
            client_slug: slug,
            business_type: businessType,
            max_devices: maxDevices,
            expires_at: expiry ? `${expiry}T00:00:00.000Z` : null,
          });
        }}
      >
        <label>
          {t('licenses.issue.code')}
          <textarea
            className="mono"
            rows={3}
            value={code}
            disabled={!enabled}
            onChange={(e) => {
              setCode(e.target.value);
            }}
            onBlur={() => {
              if (code.trim()) decode.mutate(code);
            }}
          />
        </label>
        {decode.data && (
          <dl className="facts">
            <dt>{t('licenses.issue.device')}</dt>
            <dd>{decode.data.device_name}</dd>
            <dt>{t('licenses.issue.client')}</dt>
            <dd>
              <code>{decode.data.client_id}</code>
            </dd>
            <dt>{t('licenses.issue.fingerprint')}</dt>
            <dd>
              <code>{decode.data.fingerprint.slice(0, 16)}…</code>
            </dd>
          </dl>
        )}
        {decode.error && (
          <p role="alert" className="error">
            {decode.error.message}
          </p>
        )}
        <div className="grid">
          <label>
            {t('licenses.issue.slug')}
            <input
              value={slug}
              disabled={!enabled}
              placeholder="acme-retail"
              onChange={(e) => {
                setSlug(e.target.value);
              }}
            />
          </label>
          <label>
            {t('licenses.issue.businessType')}
            <select
              value={businessType}
              disabled={!enabled}
              onChange={(e) => {
                setBusinessType(e.target.value as BusinessType);
              }}
            >
              {BUSINESS_TYPES.map((type) => (
                <option key={type} value={type}>
                  {t(`licenses.businessType.${type}`)}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t('licenses.issue.maxDevices')}
            <input
              type="number"
              min={1}
              max={1000}
              value={maxDevices}
              disabled={!enabled}
              onChange={(e) => {
                setMaxDevices(Math.max(1, Math.trunc(Number(e.target.value) || 1)));
              }}
            />
          </label>
          <label>
            {t('licenses.issue.expiry')}
            <input
              type="date"
              value={expiry}
              disabled={!enabled}
              onChange={(e) => {
                setExpiry(e.target.value);
              }}
            />
          </label>
        </div>
        {issue.error && (
          <p role="alert" className="error">
            {issue.error.message}
          </p>
        )}
        <button
          type="submit"
          className="button button--primary"
          disabled={!enabled || !code.trim() || !slug || issue.isPending}
        >
          {issue.isPending ? t('licenses.working') : t('licenses.issue.submit')}
        </button>
      </form>

      {issue.data && (
        <div className="form">
          <p className="muted">{t('licenses.issue.result')}</p>
          <textarea className="mono" readOnly rows={6} value={issue.data.token} />
          <CopyButton text={issue.data.token} />
        </div>
      )}
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
