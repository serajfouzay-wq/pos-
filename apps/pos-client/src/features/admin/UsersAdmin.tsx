import { ROLES, type Role } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useCreateUser, useUsers } from '../../ipc/queries';

export function UsersAdmin() {
  const { t } = useTranslation();
  const users = useUsers();
  const create = useCreateUser();
  const [name, setName] = useState('');
  const [role, setRole] = useState<Role>('cashier');
  const [pin, setPin] = useState('');

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('admin.users.title')}</h1>
      </header>
      <form
        className="card form-grid"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate(
            { display_name: name.trim(), role, pin },
            {
              onSuccess: () => {
                setName('');
                setPin('');
              },
            },
          );
        }}
      >
        <label className="field">
          {t('admin.users.name')}
          <input
            value={name}
            required
            maxLength={80}
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </label>
        <label className="field">
          {t('admin.users.role')}
          <select
            value={role}
            onChange={(e) => {
              setRole(e.target.value as Role);
            }}
          >
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {t(`roles.${r}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          {t('admin.users.pin')}
          <input
            value={pin}
            type="password"
            inputMode="numeric"
            dir="ltr"
            pattern="[0-9]{4,6}"
            maxLength={6}
            required
            onChange={(e) => {
              setPin(e.target.value.replace(/\D/g, ''));
            }}
          />
        </label>
        {create.error && (
          <p role="alert" className="error-text span-all">
            {create.error.message}
          </p>
        )}
        <div className="row span-all">
          <button
            type="submit"
            className="button button--primary"
            disabled={create.isPending || pin.length < 4 || !name.trim()}
          >
            {t('admin.users.add')}
          </button>
        </div>
      </form>
      <table className="table">
        <thead>
          <tr>
            <th>{t('admin.users.name')}</th>
            <th>{t('admin.users.role')}</th>
            <th>{t('admin.users.status')}</th>
          </tr>
        </thead>
        <tbody>
          {users.data?.map((u) => (
            <tr key={u.id}>
              <td>{u.display_name}</td>
              <td>{t(`roles.${u.role}`)}</td>
              <td>
                {u.locked_until
                  ? t('admin.users.locked')
                  : u.is_active
                    ? t('admin.users.active')
                    : t('admin.users.inactive')}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
