import { useTranslation } from 'react-i18next';
import { ErrorText } from '../../components/ErrorText';
import { useBuilds } from '../../ipc/queries';
import { BuildList } from './BuildList';

export function BuildsScreen() {
  const { t } = useTranslation();
  const builds = useBuilds(null);
  return (
    <div className="stack stack--wide">
      <p className="muted">{t('builds.intro')}</p>
      <section className="card">
        <ErrorText error={builds.error} />
        {builds.data && <BuildList builds={builds.data} showClient />}
      </section>
    </div>
  );
}
