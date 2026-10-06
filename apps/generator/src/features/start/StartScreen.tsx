import { isActiveBuild } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import {
  useBuilds,
  useBuildSettings,
  useClients,
  useSigningKey,
  useUpdateKey,
} from '../../ipc/queries';
import { useNavigationStore } from '../../stores/navigation';
import { STEP_IDS, type StepId, stepStates, type SetupFacts } from './steps';

/** How the pieces fit: this PC → GitHub → the shop's PC → activation. */
function HowItWorks() {
  const { t } = useTranslation();
  const nodes = ['pc', 'github', 'shop', 'license'] as const;
  return (
    <section className="card">
      <h2>{t('start.how.title')}</h2>
      <ol className="flow" aria-label={t('start.how.title')}>
        {nodes.map((node, i) => (
          <li key={node} className="flow__item">
            <div className={`flow__node flow__node--${node}`}>
              <span className="flow__number">{i + 1}</span>
              <strong>{t(`start.how.${node}.title`)}</strong>
              <span className="muted small">{t(`start.how.${node}.text`)}</span>
            </div>
            {i < nodes.length - 1 && (
              <span className="flow__arrow" aria-hidden="true">
                →
              </span>
            )}
          </li>
        ))}
      </ol>
      <p className="muted small">{t('start.how.updates')}</p>
    </section>
  );
}

/** The first-run guide: what to do, in order, ticked off as it happens. */
export function StartScreen() {
  const { t } = useTranslation();
  const key = useSigningKey();
  const settings = useBuildSettings();
  const clients = useClients();
  const builds = useBuilds(null);
  const updateKey = useUpdateKey();
  const { navigate, openClient, startNewClient } = useNavigationStore();

  const list = builds.data ?? [];
  const facts: SetupFacts = {
    hasLicenseKey: key.data !== undefined && key.data.state !== 'absent',
    githubConnected: Boolean(settings.data?.repo_owner && settings.data.token_configured),
    clientCount: clients.data?.length ?? 0,
    buildRunning: list.some((b) => isActiveBuild(b.status)),
    buildReady: list.some((b) => b.status === 'succeeded'),
    downloaded: list.some((b) => b.download_path !== null),
    licensed: (clients.data ?? []).some((c) => c.license_count > 0),
    keysBackedUp: Boolean(updateKey.data?.backed_up_at),
  };
  const states = stepStates(facts);
  const done = STEP_IDS.filter((id) => states[id] === 'done').length;
  const firstClient = clients.data?.[0]?.client_id;

  const go: Record<StepId, () => void> = {
    licenseKey: () => {
      navigate('licenses');
    },
    github: () => {
      navigate('settings');
    },
    client: startNewClient,
    build: () => {
      if (firstClient) openClient(firstClient, 'builds');
      else startNewClient();
    },
    download: () => {
      navigate('builds');
    },
    activate: () => {
      navigate('licenses');
    },
    backup: () => {
      navigate('settings');
    },
  };

  return (
    <div className="stack stack--wide">
      <p className="muted">{t('start.intro')}</p>
      <HowItWorks />
      <section className="card">
        <div className="row row--between">
          <h2>{t('start.steps.title')}</h2>
          <span className={done === STEP_IDS.length ? 'badge badge--ok' : 'badge'}>
            {t('start.steps.progress', { done, total: STEP_IDS.length })}
          </span>
        </div>
        {done === STEP_IDS.length && <p className="ok-text">{t('start.steps.allDone')}</p>}
        <ol className="steps">
          {STEP_IDS.map((id, i) => {
            const state = states[id];
            return (
              <li key={id} className={`step step--${state}`}>
                <span className="step__mark" aria-hidden="true">
                  {state === 'done' ? '✓' : i + 1}
                </span>
                <div className="step__body">
                  <strong>{t(`start.steps.${id}.title`)}</strong>
                  <span className="muted">{t(`start.steps.${id}.text`)}</span>
                  {state === 'waiting' && (
                    <span className="small">{t('start.steps.build.waiting')}</span>
                  )}
                </div>
                <button
                  type="button"
                  className={state === 'next' ? 'button button--primary' : 'button'}
                  onClick={go[id]}
                >
                  {state === 'done' ? t('start.steps.open') : t(`start.steps.${id}.action`)}
                </button>
              </li>
            );
          })}
        </ol>
      </section>
    </div>
  );
}
