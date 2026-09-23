import type { LoginUser } from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PinPad } from '../../components/PinPad';
import { useAppInfo, useLogin, useLoginUsers } from '../../ipc/queries';

/** Tap your name, enter your PIN. No keyboard required. */
export function LoginScreen() {
  const { t } = useTranslation();
  const info = useAppInfo();
  const users = useLoginUsers(true);
  const login = useLogin();
  const [selected, setSelected] = useState<LoginUser | null>(null);

  return (
    <main className="shell">
      <section className="shell__card login">
        <h1>{info.data?.client.display_name}</h1>
        <AnimatePresence mode="wait">
          {!selected ? (
            <motion.div
              key="users"
              className="login__users"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <p className="shell__muted">{t('session.whoIsThere')}</p>
              <div className="user-tiles">
                {users.data?.map((user) => (
                  <button
                    key={user.id}
                    type="button"
                    className="user-tile"
                    onClick={() => {
                      login.reset();
                      setSelected(user);
                    }}
                  >
                    <span className="user-tile__avatar" aria-hidden>
                      {user.display_name.slice(0, 1).toUpperCase()}
                    </span>
                    <span>{user.display_name}</span>
                    <span className="muted small">{t(`roles.${user.role}`)}</span>
                  </button>
                ))}
              </div>
            </motion.div>
          ) : (
            <motion.div
              key="pin"
              initial={{ opacity: 0, x: 24 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0 }}
            >
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setSelected(null);
                }}
              >
                ← {t('session.notYou')}
              </button>
              <p className="login__name">
                {selected.display_name} ·{' '}
                <span className="muted">{t(`roles.${selected.role}`)}</span>
              </p>
              <PinPad
                busy={login.isPending}
                error={login.error?.message}
                onSubmit={(pin) => {
                  login.mutate({ user_id: selected.id, pin });
                }}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </section>
    </main>
  );
}
