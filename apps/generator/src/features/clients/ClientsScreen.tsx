import {
  BUSINESS_TYPES,
  CURRENCY_CODES,
  NewClientInputSchema,
  type BusinessType,
  type ClientSummary,
  type CurrencyCode,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { BuildStatusBadge } from '../../components/BuildStatusBadge';
import { ErrorText } from '../../components/ErrorText';
import { useClients, useCreateClient } from '../../ipc/queries';
import { formatDateTime, slugify } from '../../lib/format';
import { useNavigationStore } from '../../stores/navigation';
import { ClientEditor } from './ClientEditor';

function NewClientForm({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation();
  const create = useCreateClient();
  const openClient = useNavigationStore((s) => s.openClient);
  const [name, setName] = useState('');
  const [slug, setSlug] = useState('');
  const [slugEdited, setSlugEdited] = useState(false);
  const [businessType, setBusinessType] = useState<BusinessType>('retail');
  const [currency, setCurrency] = useState<CurrencyCode>('KWD');

  const input = {
    display_name: name,
    client_slug: slugEdited ? slug : slugify(name),
    business_type: businessType,
    base_currency: currency,
  };
  const valid = NewClientInputSchema.safeParse(input).success;

  return (
    <form
      className="card form"
      onSubmit={(e) => {
        e.preventDefault();
        create.mutate(input, {
          onSuccess: (detail) => {
            onDone();
            openClient(detail.client_id);
          },
        });
      }}
    >
      <h2>{t('clients.new.title')}</h2>
      <div className="grid">
        <label>
          {t('clients.fields.displayName')}
          <input
            autoFocus
            value={name}
            maxLength={80}
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </label>
        <label>
          {t('clients.fields.slug')}
          <input
            className="mono"
            value={input.client_slug}
            maxLength={40}
            onChange={(e) => {
              setSlugEdited(true);
              setSlug(e.target.value);
            }}
          />
        </label>
        <label>
          {t('clients.fields.businessType')}
          <select
            value={businessType}
            onChange={(e) => {
              setBusinessType(e.target.value as BusinessType);
            }}
          >
            {BUSINESS_TYPES.map((type) => (
              <option key={type} value={type}>
                {t(`businessType.${type}`)}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t('clients.fields.baseCurrency')}
          <select
            value={currency}
            onChange={(e) => {
              setCurrency(e.target.value as CurrencyCode);
            }}
          >
            {CURRENCY_CODES.map((code) => (
              <option key={code} value={code}>
                {code}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="muted">{t('clients.new.slugHelp')}</p>
      <ErrorText error={create.error} />
      <div className="row">
        <button
          type="submit"
          className="button button--primary"
          disabled={!valid || create.isPending}
        >
          {t('clients.new.create')}
        </button>
        <button type="button" className="button" onClick={onDone}>
          {t('common.cancel')}
        </button>
      </div>
    </form>
  );
}

function ClientCard({ client }: { client: ClientSummary }) {
  const { t, i18n } = useTranslation();
  const openClient = useNavigationStore((s) => s.openClient);
  return (
    <button
      type="button"
      className="client-card"
      onClick={() => {
        openClient(client.client_id);
      }}
    >
      <span className="client-card__name">{client.display_name}</span>
      <code className="muted">{client.client_slug}</code>
      <span className="client-card__meta">
        <span className="badge">{t(`businessType.${client.business_type}`)}</span>
        {client.has_receipt_logo && <span className="badge">{t('clients.card.logo')}</span>}
        {client.has_app_icon && <span className="badge">{t('clients.card.icon')}</span>}
        <span className="badge">{t('clients.card.licenses', { count: client.license_count })}</span>
      </span>
      <span className="client-card__meta">
        {client.last_build ? (
          <>
            <BuildStatusBadge status={client.last_build.status} />
            <span className="muted">
              {formatDateTime(client.last_build.requested_at, i18n.language)}
            </span>
          </>
        ) : (
          <span className="muted">{t('clients.card.neverBuilt')}</span>
        )}
      </span>
    </button>
  );
}

export function ClientsScreen() {
  const { t } = useTranslation();
  const clients = useClients();
  const clientId = useNavigationStore((s) => s.clientId);
  const [creating, setCreating] = useState(false);

  if (clientId) return <ClientEditor key={clientId} clientId={clientId} />;

  return (
    <div className="stack stack--wide">
      <div className="row row--between">
        <p className="muted">{t('clients.intro')}</p>
        {!creating && (
          <button
            type="button"
            className="button button--primary"
            onClick={() => {
              setCreating(true);
            }}
          >
            {t('clients.new.open')}
          </button>
        )}
      </div>
      {creating && (
        <NewClientForm
          onDone={() => {
            setCreating(false);
          }}
        />
      )}
      <ErrorText error={clients.error} />
      {clients.data?.length === 0 && !creating && (
        <div className="placeholder">{t('clients.empty')}</div>
      )}
      <div className="client-grid">
        {clients.data?.map((client) => (
          <ClientCard key={client.client_id} client={client} />
        ))}
      </div>
    </div>
  );
}
