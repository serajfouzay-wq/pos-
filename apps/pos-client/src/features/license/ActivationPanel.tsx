import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useActivateLicense, useActivationRequest } from '../../ipc/queries';

/**
 * Offline activation: the till shows its request code, the operator pastes it
 * into the generator and brings back a signed license token.
 */
export function ActivationPanel() {
  const { t } = useTranslation();
  const request = useActivationRequest(true);
  const activate = useActivateLicense();
  const [token, setToken] = useState('');
  const [copied, setCopied] = useState(false);

  const rejection =
    activate.data && activate.data.state !== 'valid' ? activate.data.reason : undefined;

  return (
    <div className="activation">
      <section>
        <h2>{t('license.activation.step1')}</h2>
        {request.data ? (
          <>
            <p className="shell__muted">
              {t('license.activation.device', { name: request.data.device_name })}
            </p>
            <textarea
              className="activation__code"
              readOnly
              rows={4}
              value={request.data.code}
              onFocus={(e) => {
                e.currentTarget.select();
              }}
            />
            <button
              type="button"
              className="button"
              onClick={() => {
                void navigator.clipboard.writeText(request.data.code).then(() => {
                  setCopied(true);
                });
              }}
            >
              {copied ? t('license.activation.copied') : t('license.activation.copy')}
            </button>
          </>
        ) : (
          <p className="shell__muted">
            {request.isError ? request.error.message : t('license.checking')}
          </p>
        )}
      </section>

      <form
        onSubmit={(e) => {
          e.preventDefault();
          activate.mutate(token.trim());
        }}
      >
        <h2>{t('license.activation.step2')}</h2>
        <textarea
          className="activation__code"
          rows={5}
          placeholder={t('license.activation.tokenPlaceholder')}
          value={token}
          onChange={(e) => {
            setToken(e.target.value);
          }}
          spellCheck={false}
        />
        {(rejection ?? activate.error?.message) && (
          <p role="alert" className="shell__status--error">
            {rejection ?? activate.error?.message}
          </p>
        )}
        <button
          type="submit"
          className="button button--primary"
          disabled={token.trim().length === 0 || activate.isPending}
        >
          {activate.isPending ? t('license.activation.activating') : t('license.activation.submit')}
        </button>
      </form>
    </div>
  );
}
