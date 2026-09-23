import { ClientConfigSchema, type ClientConfig, type ClientDetail } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ErrorText } from '../../components/ErrorText';
import { useArchiveClient, useClient, useSaveClient } from '../../ipc/queries';
import { CLIENT_TABS, useNavigationStore } from '../../stores/navigation';
import { ClientBuildsTab } from '../builds/ClientBuildsTab';
import { ClientLicensesTab } from '../licenses/ClientLicensesTab';
import { DetailsTab } from './DetailsTab';
import { ReceiptTab } from './ReceiptTab';

export interface Draft {
  config: ClientConfig;
  notes: string;
}

function draftOf(detail: ClientDetail): Draft {
  return { config: detail.config, notes: detail.notes };
}

function Editor({ detail }: { detail: ClientDetail }) {
  const { t } = useTranslation();
  const { clientTab, setClientTab, closeClient } = useNavigationStore();
  const save = useSaveClient();
  const archive = useArchiveClient();
  const [draft, setDraft] = useState<Draft>(() => draftOf(detail));
  const [base, setBase] = useState<Draft>(() => draftOf(detail));
  const [confirmArchive, setConfirmArchive] = useState(false);

  const dirty = JSON.stringify(draft) !== JSON.stringify(base);

  // The server changed the record (save, logo upload): adopt it, keeping
  // unsaved edits but taking the fields only the server decides.
  const [seen, setSeen] = useState(detail);
  if (seen !== detail) {
    const incoming = draftOf(detail);
    setSeen(detail);
    setBase(incoming);
    setDraft(
      JSON.stringify(draft) === JSON.stringify(base)
        ? incoming
        : {
            ...draft,
            config: {
              ...draft.config,
              receipt: { ...draft.config.receipt, logo_asset: detail.config.receipt.logo_asset },
            },
          },
    );
  }

  const validation = ClientConfigSchema.safeParse(draft.config);
  const firstIssue = validation.error?.issues[0];

  return (
    <div className="stack stack--wide">
      <div className="row row--between">
        <div>
          <button type="button" className="link" onClick={closeClient}>
            {t('clients.editor.back')}
          </button>
          <h2 className="editor__title">
            {draft.config.display_name || detail.config.display_name}
          </h2>
          <code className="muted">
            {detail.config.client_slug} · {detail.client_id}
          </code>
        </div>
        <div className="row">
          {dirty && (
            <button
              type="button"
              className="button"
              onClick={() => {
                setDraft(base);
              }}
            >
              {t('clients.editor.discard')}
            </button>
          )}
          <button
            type="button"
            className="button button--primary"
            disabled={!dirty || !validation.success || save.isPending}
            onClick={() => {
              save.mutate({ clientId: detail.client_id, config: draft.config, notes: draft.notes });
            }}
          >
            {save.isPending ? t('common.working') : t('common.save')}
          </button>
        </div>
      </div>

      {firstIssue && (
        <p role="alert" className="warning">
          {t('clients.editor.invalid', {
            field: firstIssue.path.join('.'),
            message: firstIssue.message,
          })}
        </p>
      )}
      <ErrorText error={save.error} />

      <div className="tabs" role="tablist">
        {CLIENT_TABS.map((tab) => (
          <button
            key={tab}
            type="button"
            role="tab"
            aria-selected={tab === clientTab}
            className="tab"
            onClick={() => {
              setClientTab(tab);
            }}
          >
            {t(`clients.tabs.${tab}`)}
          </button>
        ))}
      </div>

      {clientTab === 'details' && <DetailsTab draft={draft} onChange={setDraft} />}
      {clientTab === 'receipt' && <ReceiptTab detail={detail} draft={draft} onChange={setDraft} />}
      {clientTab === 'licenses' && <ClientLicensesTab detail={detail} />}
      {clientTab === 'builds' && <ClientBuildsTab detail={detail} unsaved={dirty} />}

      {clientTab === 'details' && (
        <section className="card card--danger">
          <h3>{t('clients.archive.title')}</h3>
          <p className="muted">{t('clients.archive.help')}</p>
          {confirmArchive ? (
            <div className="row">
              <button
                type="button"
                className="button button--danger"
                disabled={archive.isPending}
                onClick={() => {
                  archive.mutate(detail.client_id, { onSuccess: closeClient });
                }}
              >
                {t('clients.archive.confirm', { name: detail.config.display_name })}
              </button>
              <button
                type="button"
                className="button"
                onClick={() => {
                  setConfirmArchive(false);
                }}
              >
                {t('common.cancel')}
              </button>
            </div>
          ) : (
            <button
              type="button"
              className="button"
              onClick={() => {
                setConfirmArchive(true);
              }}
            >
              {t('clients.archive.open')}
            </button>
          )}
          <ErrorText error={archive.error} />
        </section>
      )}
    </div>
  );
}

export function ClientEditor({ clientId }: { clientId: string }) {
  const client = useClient(clientId);
  if (client.error) return <ErrorText error={client.error} />;
  if (!client.data) return null;
  return <Editor detail={client.data} />;
}
