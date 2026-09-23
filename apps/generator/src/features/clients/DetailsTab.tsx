import {
  BUSINESS_TYPES,
  CURRENCY_CODES,
  LOCALES,
  type BusinessType,
  type ClientConfig,
  type CurrencyCode,
  type Locale,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { formatBps, parseBps } from '../../lib/format';
import type { Draft } from './ClientEditor';

interface Props {
  draft: Draft;
  onChange: (draft: Draft) => void;
}

const FEATURES = ['loyalty', 'kitchen_display', 'multi_currency', 'purchase_orders'] as const;
const LOCALE_NAMES: Record<Locale, string> = { en: 'English', ar: 'العربية' };

function toggle<T>(list: readonly T[], value: T, on: boolean): T[] {
  return on ? [...list.filter((v) => v !== value), value] : list.filter((v) => v !== value);
}

export function DetailsTab({ draft, onChange }: Props) {
  const { t } = useTranslation();
  const config = draft.config;
  const set = (patch: Partial<ClientConfig>) => {
    onChange({ ...draft, config: { ...config, ...patch } });
  };
  const [rateText, setRateText] = useState(formatBps(config.tax.default_rate_bps));
  const rateValid = parseBps(rateText) !== null;

  return (
    <div className="stack">
      <section className="card form">
        <h3>{t('clients.sections.identity')}</h3>
        <div className="grid">
          <label>
            {t('clients.fields.displayName')}
            <input
              value={config.display_name}
              maxLength={80}
              onChange={(e) => {
                set({ display_name: e.target.value });
              }}
            />
          </label>
          <label>
            {t('clients.fields.businessType')}
            <select
              value={config.business_type}
              onChange={(e) => {
                set({ business_type: e.target.value as BusinessType });
              }}
            >
              {BUSINESS_TYPES.map((type) => (
                <option key={type} value={type}>
                  {t(`businessType.${type}`)}
                </option>
              ))}
            </select>
          </label>
        </div>
        <p className="muted">{t('clients.fields.slugFixed')}</p>
      </section>

      <section className="card form">
        <h3>{t('clients.sections.language')}</h3>
        <div className="row">
          {LOCALES.map((locale) => (
            <label key={locale} className="check">
              <input
                type="checkbox"
                checked={config.locale.supported.includes(locale)}
                onChange={(e) => {
                  const supported = toggle(config.locale.supported, locale, e.target.checked);
                  set({
                    locale: {
                      supported,
                      default: supported.includes(config.locale.default)
                        ? config.locale.default
                        : (supported[0] ?? config.locale.default),
                    },
                  });
                }}
              />
              {LOCALE_NAMES[locale]}
            </label>
          ))}
        </div>
        <label>
          {t('clients.fields.defaultLocale')}
          <select
            value={config.locale.default}
            onChange={(e) => {
              set({ locale: { ...config.locale, default: e.target.value as Locale } });
            }}
          >
            {config.locale.supported.map((locale) => (
              <option key={locale} value={locale}>
                {LOCALE_NAMES[locale]}
              </option>
            ))}
          </select>
        </label>
      </section>

      <section className="card form">
        <h3>{t('clients.sections.money')}</h3>
        <div className="grid">
          <label>
            {t('clients.fields.baseCurrency')}
            <select
              value={config.currency.base}
              onChange={(e) => {
                const base = e.target.value as CurrencyCode;
                set({
                  currency: { base, accepted: config.currency.accepted.filter((c) => c !== base) },
                });
              }}
            >
              {CURRENCY_CODES.map((code) => (
                <option key={code} value={code}>
                  {code}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t('clients.fields.taxRate')}
            <input
              inputMode="decimal"
              value={rateText}
              aria-invalid={!rateValid}
              onChange={(e) => {
                setRateText(e.target.value);
                const bps = parseBps(e.target.value);
                if (bps !== null) set({ tax: { ...config.tax, default_rate_bps: bps } });
              }}
            />
          </label>
          <label>
            {t('clients.fields.taxNumber')}
            <input
              value={config.tax.registration_number ?? ''}
              maxLength={40}
              onChange={(e) => {
                set({
                  tax: {
                    ...config.tax,
                    registration_number: e.target.value.trim() ? e.target.value : null,
                  },
                });
              }}
            />
          </label>
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={config.tax.prices_include_tax}
            onChange={(e) => {
              set({ tax: { ...config.tax, prices_include_tax: e.target.checked } });
            }}
          />
          {t('clients.fields.pricesIncludeTax')}
        </label>
        <fieldset className="fieldset">
          <legend>{t('clients.fields.acceptedCurrencies')}</legend>
          <div className="chips">
            {CURRENCY_CODES.filter((c) => c !== config.currency.base).map((code) => (
              <label key={code} className="check">
                <input
                  type="checkbox"
                  checked={config.currency.accepted.includes(code)}
                  onChange={(e) => {
                    set({
                      currency: {
                        ...config.currency,
                        accepted: toggle(config.currency.accepted, code, e.target.checked),
                      },
                    });
                  }}
                />
                {code}
              </label>
            ))}
          </div>
        </fieldset>
      </section>

      <section className="card form">
        <h3>{t('clients.sections.features')}</h3>
        <div className="grid">
          {FEATURES.map((feature) => (
            <label key={feature} className="check">
              <input
                type="checkbox"
                checked={config.features[feature]}
                onChange={(e) => {
                  set({ features: { ...config.features, [feature]: e.target.checked } });
                }}
              />
              {t(`clients.features.${feature}`)}
            </label>
          ))}
        </div>
      </section>

      <section className="card form">
        <h3>{t('clients.sections.cloud')}</h3>
        <p className="muted">{t('clients.fields.cloudHelp')}</p>
        <div className="grid">
          <label>
            {t('clients.fields.supabaseUrl')}
            <input
              className="mono"
              placeholder="https://xyz.supabase.co"
              value={config.cloud.supabase_url ?? ''}
              onChange={(e) => {
                set({ cloud: { ...config.cloud, supabase_url: e.target.value.trim() || null } });
              }}
            />
          </label>
          <label>
            {t('clients.fields.supabaseKey')}
            <input
              className="mono"
              value={config.cloud.supabase_anon_key ?? ''}
              onChange={(e) => {
                set({
                  cloud: { ...config.cloud, supabase_anon_key: e.target.value.trim() || null },
                });
              }}
            />
          </label>
        </div>
      </section>

      <section className="card form">
        <h3>{t('clients.sections.notes')}</h3>
        <textarea
          rows={4}
          maxLength={2000}
          value={draft.notes}
          placeholder={t('clients.fields.notesHelp')}
          onChange={(e) => {
            onChange({ ...draft, notes: e.target.value });
          }}
        />
      </section>
    </div>
  );
}
