import type { ClientDetail } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../components/CopyButton';
import { ErrorText } from '../../components/ErrorText';
import { useIssuedLicenses, useSigningKey } from '../../ipc/queries';
import { formatDateTime } from '../../lib/format';
import { useNavigationStore } from '../../stores/navigation';
import { IssueLicenseForm } from './IssueLicenseForm';

export function ClientLicensesTab({ detail }: { detail: ClientDetail }) {
  const { t, i18n } = useTranslation();
  const key = useSigningKey();
  const licenses = useIssuedLicenses(detail.client_id);
  const navigate = useNavigationStore((s) => s.navigate);
  const unlocked = key.data?.state === 'unlocked';

  return (
    <div className="stack">
      <section className="card">
        <h3>{t('licenses.issue.title')}</h3>
        {!unlocked && (
          <p className="muted">
            {t('licenses.issue.unlockFirst')}{' '}
            <button
              type="button"
              className="link"
              onClick={() => {
                navigate('licenses');
              }}
            >
              {t('licenses.issue.goToKey')}
            </button>
          </p>
        )}
        <IssueLicenseForm clientId={detail.client_id} enabled={unlocked} />
      </section>

      <section className="card">
        <h3>{t('licenses.history.title')}</h3>
        <ErrorText error={licenses.error} />
        {licenses.data?.length === 0 && <p className="muted">{t('licenses.history.empty')}</p>}
        {licenses.data && licenses.data.length > 0 && (
          <table className="table">
            <thead>
              <tr>
                <th>{t('licenses.history.device')}</th>
                <th>{t('licenses.history.issued')}</th>
                <th>{t('licenses.history.expires')}</th>
                <th>{t('licenses.history.seats')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {licenses.data.map((license) => (
                <tr key={license.license_id}>
                  <td>
                    {license.device_name}
                    <br />
                    <code className="muted">{license.fingerprint_hash.slice(0, 12)}…</code>
                  </td>
                  <td>{formatDateTime(license.issued_at, i18n.language)}</td>
                  <td>
                    {license.expires_at
                      ? formatDateTime(license.expires_at, i18n.language)
                      : t('licenses.history.never')}
                  </td>
                  <td>{license.max_devices}</td>
                  <td>
                    <CopyButton text={license.token} label={t('licenses.history.copyToken')} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </div>
  );
}
