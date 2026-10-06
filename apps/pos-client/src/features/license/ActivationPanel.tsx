import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useActivateLicense,
  useActivateLicenseFile,
  useActivationRequest,
  useFoundLicenseFiles,
  useSaveActivationFile,
} from '../../ipc/queries';

const fileName = (file: string) => file.split(/[\\/]/).pop() ?? file;

/**
 * Offline activation: the till shows its request code, the operator takes it
 * to the generator and brings back a signed license, either as text or as
 * files on a USB stick (`.posactivate` out, `.poslicense` back).
 */
export function ActivationPanel() {
  const { t } = useTranslation();
  const request = useActivationRequest(true);
  const activate = useActivateLicense();
  const saveFile = useSaveActivationFile();
  const licenseFiles = useFoundLicenseFiles();
  const activateFile = useActivateLicenseFile();
  const [token, setToken] = useState('');
  const [copied, setCopied] = useState(false);

  const result = activateFile.data ?? activate.data;
  const rejection = result && result.state !== 'valid' ? result.reason : undefined;
  const error = activateFile.error ?? activate.error;

  const saveToUsb = async (deviceName: string) => {
    const chosen = await saveDialog({
      title: t('license.activation.saveFile'),
      defaultPath: `${deviceName.replace(/[\\/:*?"<>|]/g, '-')}.posactivate`,
      filters: [{ name: t('license.activation.fileKind'), extensions: ['posactivate'] }],
    });
    if (typeof chosen === 'string') saveFile.mutate(chosen);
  };

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
            <button
              type="button"
              className="button"
              disabled={saveFile.isPending}
              onClick={() => {
                void saveToUsb(request.data.device_name);
              }}
            >
              {t('license.activation.saveFile')}
            </button>
            {saveFile.data && (
              <p className="shell__muted">
                {t('license.activation.saved')} <span dir="ltr">{saveFile.data}</span>
              </p>
            )}
            {saveFile.error && (
              <p role="alert" className="shell__status--error">
                {saveFile.error.message}
              </p>
            )}
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
        {licenseFiles.data && licenseFiles.data.length > 0 ? (
          <div className="activation__files">
            <span className="shell__muted">{t('license.activation.filesFound')}</span>
            {licenseFiles.data.map((file) => (
              <button
                key={file}
                type="button"
                className="button button--primary update-file"
                disabled={activateFile.isPending}
                onClick={() => {
                  activateFile.mutate(file);
                }}
              >
                <strong dir="ltr">{fileName(file)}</strong>
                <span className="small" dir="ltr">
                  {file}
                </span>
              </button>
            ))}
          </div>
        ) : (
          <p className="shell__muted">{t('license.activation.noFile')}</p>
        )}
        <p className="shell__muted">{t('license.activation.orPaste')}</p>
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
        {(rejection ?? error?.message) && (
          <p role="alert" className="shell__status--error">
            {rejection ?? error?.message}
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
