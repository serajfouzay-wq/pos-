import {
  ClientConfigSchema,
  type AssetKind,
  type ClientConfig,
  type ClientDetail,
} from '@pos/shared';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ErrorText } from '../../components/ErrorText';
import { useAssetData, useReceiptPreview, useRemoveAsset, useUploadAsset } from '../../ipc/queries';
import { fileToBase64, formatBytes, linesFromText } from '../../lib/format';
import type { Draft } from './ClientEditor';

interface Props {
  detail: ClientDetail;
  draft: Draft;
  onChange: (draft: Draft) => void;
}

function useDebounced<T>(value: T, ms: number): T {
  const [current, setCurrent] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setCurrent(value);
    }, ms);
    return () => {
      window.clearTimeout(timer);
    };
  }, [value, ms]);
  return current;
}

function AssetCard({ detail, kind }: { detail: ClientDetail; kind: AssetKind }) {
  const { t } = useTranslation();
  const upload = useUploadAsset();
  const remove = useRemoveAsset();
  const input = useRef<HTMLInputElement>(null);
  const [readError, setReadError] = useState<Error | null>(null);
  const asset = detail.assets.find((a) => a.kind === kind) ?? null;
  const data = useAssetData(detail.client_id, kind, asset?.sha256 ?? null);

  return (
    <div className="asset">
      <div className="asset__thumb">
        {asset && data.data ? (
          <img src={`data:image/png;base64,${data.data}`} alt={t(`clients.assets.${kind}.title`)} />
        ) : (
          <span className="muted">{t('clients.assets.none')}</span>
        )}
      </div>
      <div className="asset__body">
        <strong>{t(`clients.assets.${kind}.title`)}</strong>
        <span className="muted">{t(`clients.assets.${kind}.help`)}</span>
        {asset && (
          <span className="muted">
            {asset.width}×{asset.height} · {formatBytes(asset.byte_length)}
          </span>
        )}
        <input
          ref={input}
          type="file"
          accept="image/png"
          hidden
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = '';
            if (!file) return;
            setReadError(null);
            fileToBase64(file)
              .then((dataBase64) => {
                upload.mutate({ clientId: detail.client_id, kind, dataBase64 });
              })
              .catch((error: unknown) => {
                setReadError(error instanceof Error ? error : new Error(String(error)));
              });
          }}
        />
        <div className="row">
          <button
            type="button"
            className="button"
            disabled={upload.isPending}
            onClick={() => input.current?.click()}
          >
            {asset ? t('clients.assets.replace') : t('clients.assets.upload')}
          </button>
          {asset && (
            <button
              type="button"
              className="button"
              disabled={remove.isPending}
              onClick={() => {
                remove.mutate({ clientId: detail.client_id, kind });
              }}
            >
              {t('clients.assets.remove')}
            </button>
          )}
        </div>
        <ErrorText error={readError ?? upload.error ?? remove.error} />
      </div>
    </div>
  );
}

function Preview({ detail, config }: { detail: ClientDetail; config: ClientConfig }) {
  const { t } = useTranslation();
  const debounced = useDebounced(config, 300);
  const valid = ClientConfigSchema.safeParse(debounced).success;
  const logoSha = detail.assets.find((a) => a.kind === 'receipt_logo')?.sha256 ?? null;
  const preview = useReceiptPreview(detail.client_id, debounced, logoSha);
  const data = valid ? preview.data : undefined;

  return (
    <aside className="receipt-preview" aria-label={t('clients.receipt.preview')}>
      <span className="muted">
        {t('clients.receipt.previewHelp', { width: config.receipt.paper_width_mm })}
      </span>
      <div
        className="receipt-paper"
        style={{ inlineSize: `${String((data?.columns ?? 48) + 4)}ch` }}
        dir="ltr"
      >
        {data?.logo_png_base64 && (
          <img
            className="receipt-paper__logo"
            src={`data:image/png;base64,${data.logo_png_base64}`}
            alt=""
            style={{
              inlineSize: `${String(((data.logo_width ?? 0) / (data.columns === 48 ? 576 : 384)) * 100)}%`,
            }}
          />
        )}
        <pre>{data?.text.replace(/^\[logo\]\n/, '') ?? ''}</pre>
      </div>
      {preview.error && valid && <ErrorText error={preview.error} />}
    </aside>
  );
}

export function ReceiptTab({ detail, draft, onChange }: Props) {
  const { t } = useTranslation();
  const config = draft.config;
  const receipt = config.receipt;
  const setReceipt = (patch: Partial<ClientConfig['receipt']>) => {
    onChange({ ...draft, config: { ...config, receipt: { ...receipt, ...patch } } });
  };
  const setBranding = (patch: Partial<ClientConfig['branding']>) => {
    onChange({ ...draft, config: { ...config, branding: { ...config.branding, ...patch } } });
  };

  return (
    <div className="split">
      <div className="stack">
        <section className="card form">
          <h3>{t('clients.sections.receipt')}</h3>
          <label>
            {t('clients.receipt.header')}
            <textarea
              rows={4}
              value={receipt.header_lines.join('\n')}
              onChange={(e) => {
                setReceipt({ header_lines: linesFromText(e.target.value) });
              }}
            />
            <span className="muted">{t('clients.receipt.headerHelp')}</span>
          </label>
          <label>
            {t('clients.receipt.footer')}
            <textarea
              rows={2}
              maxLength={240}
              value={receipt.footer_text}
              onChange={(e) => {
                setReceipt({ footer_text: e.target.value });
              }}
            />
          </label>
          <div className="row">
            {([80, 58] as const).map((width) => (
              <label key={width} className="check">
                <input
                  type="radio"
                  name="paper"
                  checked={receipt.paper_width_mm === width}
                  onChange={() => {
                    setReceipt({ paper_width_mm: width });
                  }}
                />
                {t('clients.receipt.paper', { width })}
              </label>
            ))}
          </div>
          <label className="check">
            <input
              type="checkbox"
              checked={receipt.show_tax_number}
              onChange={(e) => {
                setReceipt({ show_tax_number: e.target.checked });
              }}
            />
            {t('clients.receipt.showTaxNumber')}
          </label>
        </section>

        <section className="card form">
          <h3>{t('clients.sections.branding')}</h3>
          <div className="row">
            {(['primary_color', 'accent_color'] as const).map((key) => (
              <label key={key} className="color">
                <input
                  type="color"
                  value={config.branding[key].toLowerCase()}
                  onChange={(e) => {
                    setBranding({ [key]: e.target.value.toUpperCase() });
                  }}
                />
                {t(`clients.branding.${key}`)} <code>{config.branding[key]}</code>
              </label>
            ))}
          </div>
          <AssetCard detail={detail} kind="receipt_logo" />
          <AssetCard detail={detail} kind="app_icon" />
          <p className="muted">{t('clients.assets.savedImmediately')}</p>
        </section>
      </div>
      <Preview detail={detail} config={config} />
    </div>
  );
}
