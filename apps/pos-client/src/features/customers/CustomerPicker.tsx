import type { Customer } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { useCustomers, useSaveCustomer } from '../../ipc/queries';

interface Props {
  open: boolean;
  onClose: () => void;
  onPick: (customer: Customer) => void;
}

/** Find a customer by name or phone, or register one on the spot. */
export function CustomerPicker({ open, onClose, onPick }: Props) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState('');
  const [phone, setPhone] = useState('');
  const results = useCustomers({ query: query.trim(), limit: 12 }, open && !adding);
  const save = useSaveCustomer();

  const close = () => {
    setQuery('');
    setAdding(false);
    save.reset();
    onClose();
  };

  return (
    <Modal open={open} title={t('customers.pickTitle')} onClose={close}>
      {adding ? (
        <form
          className="stack"
          onSubmit={(e) => {
            e.preventDefault();
            save.mutate(
              {
                id: null,
                display_name: name,
                phone: phone.trim() || null,
                email: null,
                notes: null,
              },
              {
                onSuccess: (customer) => {
                  setName('');
                  setPhone('');
                  setAdding(false);
                  onPick(customer);
                },
              },
            );
          }}
        >
          <label className="field">
            <span>{t('customers.name')}</span>
            <input
              autoFocus
              required
              maxLength={120}
              value={name}
              onChange={(e) => {
                setName(e.target.value);
              }}
            />
          </label>
          <label className="field">
            <span>{t('customers.phone')}</span>
            <input
              dir="ltr"
              inputMode="tel"
              maxLength={32}
              value={phone}
              onChange={(e) => {
                setPhone(e.target.value);
              }}
            />
          </label>
          {save.error && (
            <p role="alert" className="error-text">
              {save.error.message}
            </p>
          )}
          <div className="row row--end">
            <button
              type="button"
              className="button"
              onClick={() => {
                setAdding(false);
              }}
            >
              {t('common.back')}
            </button>
            <button
              type="submit"
              className="button button--primary"
              disabled={!name.trim() || save.isPending}
            >
              {t('customers.register')}
            </button>
          </div>
        </form>
      ) : (
        <div className="stack">
          <input
            className="search"
            type="search"
            autoFocus
            placeholder={t('customers.search')}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
          />
          <ul className="customer-list">
            {results.data?.length === 0 && (
              <li className="muted">{query ? t('customers.none') : t('customers.noneYet')}</li>
            )}
            {results.data?.map((c) => (
              <li key={c.id}>
                <button
                  type="button"
                  className="customer-list__item"
                  onClick={() => {
                    onPick(c);
                  }}
                >
                  <span className="grow">
                    <strong>{c.display_name}</strong>
                    {c.phone && (
                      <span className="muted small" dir="ltr">
                        {' '}
                        · {c.phone}
                      </span>
                    )}
                  </span>
                  <span className="badge">
                    {t('customers.points', { count: c.loyalty_points })}
                  </span>
                </button>
              </li>
            ))}
          </ul>
          <button
            type="button"
            className="button"
            onClick={() => {
              setAdding(true);
              // A typed number or name is the likely start of the new record.
              if (/^[\d+\-() ]+$/.test(query.trim())) setPhone(query.trim());
              else setName(query.trim());
            }}
          >
            + {t('customers.new')}
          </button>
        </div>
      )}
    </Modal>
  );
}
