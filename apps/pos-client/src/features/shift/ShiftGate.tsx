import type { Session, ShiftSummary } from '@pos/shared';
import { motion } from 'framer-motion';
import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { AmountPad } from '../../components/AmountPad';
import { useCurrentShift, useLogout, useOpenShift } from '../../ipc/queries';
import { can } from '../../lib/permissions';

/** Selling requires an open shift; only managers and owners can open one. */
export function ShiftGate({
  session,
  children,
}: {
  session: Session;
  children: (shift: ShiftSummary) => ReactNode;
}) {
  const { t } = useTranslation();
  const shift = useCurrentShift();
  const open = useOpenShift();
  const logout = useLogout();
  const [float, setFloat] = useState(0);

  if (shift.isPending) return <p className="shell__status center">{t('app.loading')}</p>;
  if (shift.isError)
    return (
      <p role="alert" className="error-text center">
        {shift.error.message}
      </p>
    );
  if (shift.data) return <>{children(shift.data)}</>;

  return (
    <div className="center-stage">
      <motion.section
        className="shell__card"
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
      >
        <h1>{t('shift.closedTitle')}</h1>
        {can(session, 'shift.open') ? (
          <>
            <p className="shell__muted">{t('shift.openBody')}</p>
            <AmountPad
              label={t('shift.openingFloat')}
              value={float}
              onChange={setFloat}
              onEnter={() => {
                open.mutate(float);
              }}
            />
            {open.error && (
              <p role="alert" className="error-text">
                {open.error.message}
              </p>
            )}
            <button
              type="button"
              className="button button--primary button--block"
              disabled={open.isPending}
              onClick={() => {
                open.mutate(float);
              }}
            >
              {t('shift.open')}
            </button>
          </>
        ) : (
          <>
            <p className="shell__muted">{t('shift.askManager')}</p>
            <button
              type="button"
              className="button button--block"
              onClick={() => {
                logout.mutate();
              }}
            >
              {t('session.switchUser')}
            </button>
          </>
        )}
      </motion.section>
    </div>
  );
}
