import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../components/CopyButton';
import { ErrorText } from '../../components/ErrorText';
import { useClients, useDecodeActivation, useIssueLicense } from '../../ipc/queries';

interface Props {
  /** Fixed client (client page); otherwise the operator picks one. */
  clientId?: string;
  enabled: boolean;
}

/** Activation code from a till → signed license for that till. */
export function IssueLicenseForm({ clientId, enabled }: Props) {
  const { t } = useTranslation();
  const clients = useClients();
  const decode = useDecodeActivation();
  const issue = useIssueLicense();
  const [code, setCode] = useState('');
  const [picked, setPicked] = useState('');
  const [maxDevices, setMaxDevices] = useState(1);
  const [expiry, setExpiry] = useState('');

  // A code carries the client id of the build it came from.
  const target = clientId ?? (picked !== '' ? picked : (decode.data?.client_id ?? ''));
  const targetClient = clients.data?.find((c) => c.client_id === target);
  const mismatch = decode.data && target && decode.data.client_id !== target;

  return (
    <form
      className="form"
      onSubmit={(e) => {
        e.preventDefault();
        issue.mutate({
          activation_code: code.trim(),
          client_id: target,
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
            issue.reset();
          }}
          onBlur={() => {
            if (code.trim()) decode.mutate(code.trim());
          }}
        />
      </label>
      {decode.data && (
        <dl className="facts">
          <dt>{t('licenses.issue.device')}</dt>
          <dd>{decode.data.device_name}</dd>
          <dt>{t('licenses.issue.client')}</dt>
          <dd>
            {clients.data?.find((c) => c.client_id === decode.data.client_id)?.display_name ?? (
              <code>{decode.data.client_id}</code>
            )}
          </dd>
          <dt>{t('licenses.issue.fingerprint')}</dt>
          <dd>
            <code>{decode.data.fingerprint.slice(0, 16)}…</code>
          </dd>
          <dt>{t('licenses.issue.version')}</dt>
          <dd>{decode.data.app_version}</dd>
        </dl>
      )}
      <ErrorText error={decode.error} />
      {mismatch && (
        <p role="alert" className="warning">
          {t('licenses.issue.wrongClient')}
        </p>
      )}
      <div className="grid">
        {!clientId && (
          <label>
            {t('licenses.issue.forClient')}
            <select
              value={target}
              disabled={!enabled}
              onChange={(e) => {
                setPicked(e.target.value);
              }}
            >
              <option value="">{t('licenses.issue.pickClient')}</option>
              {clients.data?.map((c) => (
                <option key={c.client_id} value={c.client_id}>
                  {c.display_name} ({c.client_slug})
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          {t('licenses.issue.maxDevices')}
          <input
            type="number"
            min={1}
            max={1000}
            value={maxDevices}
            disabled={!enabled}
            onChange={(e) => {
              setMaxDevices(Math.min(1000, Math.max(1, Math.trunc(Number(e.target.value) || 1))));
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
      <ErrorText error={issue.error} />
      <button
        type="submit"
        className="button button--primary"
        disabled={!enabled || !code.trim() || !target || Boolean(mismatch) || issue.isPending}
      >
        {issue.isPending
          ? t('common.working')
          : t('licenses.issue.submit', { name: targetClient?.display_name ?? '' })}
      </button>

      {issue.data && (
        <div className="form">
          <p className="muted">{t('licenses.issue.result')}</p>
          <textarea className="mono" readOnly rows={5} value={issue.data.token} />
          <CopyButton text={issue.data.token} />
        </div>
      )}
    </form>
  );
}
