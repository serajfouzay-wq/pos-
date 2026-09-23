import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export function CopyButton({ text, label }: { text: string; label?: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="button"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          window.setTimeout(() => {
            setCopied(false);
          }, 2_000);
        });
      }}
    >
      {copied ? t('common.copied') : (label ?? t('common.copy'))}
    </button>
  );
}
