import { motion } from 'framer-motion';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PinPad } from '../../components/PinPad';
import { useAppInfo, useBootstrapOwner } from '../../ipc/queries';

/** First run: whoever sets up the till becomes its owner. */
export function SetupOwner() {
  const { t } = useTranslation();
  const info = useAppInfo();
  const bootstrap = useBootstrapOwner();
  const [name, setName] = useState('');
  const [firstPin, setFirstPin] = useState<string | null>(null);
  const [mismatch, setMismatch] = useState(false);

  return (
    <main className="shell">
      <motion.section
        className="shell__card setup"
        initial={{ opacity: 0, y: 16 }}
        animate={{ opacity: 1, y: 0 }}
      >
        <h1>{t('session.setupTitle', { name: info.data?.client.display_name ?? '' })}</h1>
        <p className="shell__muted">{t('session.setupBody')}</p>
        <label className="field">
          {t('session.ownerName')}
          <input
            value={name}
            maxLength={80}
            autoFocus
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </label>
        <p className="shell__muted">
          {firstPin === null ? t('session.choosePin') : t('session.confirmPin')}
        </p>
        <PinPad
          busy={bootstrap.isPending}
          error={mismatch ? t('session.pinMismatch') : bootstrap.error?.message}
          submitLabel={firstPin === null ? t('common.next') : t('session.createOwner')}
          onSubmit={(pin) => {
            if (firstPin === null) {
              setFirstPin(pin);
              setMismatch(false);
            } else if (pin !== firstPin) {
              setFirstPin(null);
              setMismatch(true);
            } else if (name.trim()) {
              bootstrap.mutate({ display_name: name.trim(), pin });
            }
          }}
        />
      </motion.section>
    </main>
  );
}
