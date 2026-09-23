import type { BuildSettings, BuildSettingsInput } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ErrorText } from '../../components/ErrorText';
import {
  useBuildSettings,
  useCheckBuildSettings,
  useClearGithubToken,
  useSaveBuildSettings,
} from '../../ipc/queries';

function SettingsForm({ current }: { current: BuildSettings }) {
  const { t } = useTranslation();
  const save = useSaveBuildSettings();
  const clear = useClearGithubToken();
  const check = useCheckBuildSettings();
  const [form, setForm] = useState<BuildSettingsInput>({
    repo_owner: current.repo_owner,
    repo_name: current.repo_name,
    branch: current.branch,
    workflow_file: current.workflow_file,
    api_base_url: current.api_base_url,
  });
  const [token, setToken] = useState('');
  const field = (key: keyof BuildSettingsInput, label: string, placeholder?: string) => (
    <label>
      {label}
      <input
        className="mono"
        value={form[key]}
        placeholder={placeholder}
        onChange={(e) => {
          setForm({ ...form, [key]: e.target.value.trim() });
          check.reset();
        }}
      />
    </label>
  );

  return (
    <div className="stack">
      <form
        className="card form"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate(
            { settings: form, githubToken: token || null },
            {
              onSuccess: () => {
                setToken('');
              },
            },
          );
        }}
      >
        <h2>{t('settings.repo.title')}</h2>
        <p className="muted">{t('settings.repo.help')}</p>
        <div className="grid">
          {field('repo_owner', t('settings.repo.owner'), 'acme')}
          {field('repo_name', t('settings.repo.name'), 'pos-factory')}
          {field('branch', t('settings.repo.branch'))}
          {field('workflow_file', t('settings.repo.workflow'))}
        </div>
        {field('api_base_url', t('settings.repo.apiBase'))}
        <label>
          {t('settings.token.label')}
          <input
            type="password"
            className="mono"
            autoComplete="off"
            value={token}
            placeholder={current.token_configured ? t('settings.token.keep') : 'github_pat_…'}
            onChange={(e) => {
              setToken(e.target.value.trim());
            }}
          />
          <span className="muted">{t('settings.token.help')}</span>
        </label>
        <p className={current.token_configured ? 'muted' : 'warning'}>
          {current.token_configured ? t('settings.token.stored') : t('settings.token.missing')}
        </p>
        <ErrorText error={save.error ?? clear.error} />
        <div className="row">
          <button type="submit" className="button button--primary" disabled={save.isPending}>
            {save.isPending ? t('common.working') : t('common.save')}
          </button>
          <button
            type="button"
            className="button"
            disabled={check.isPending || !current.token_configured}
            onClick={() => {
              check.mutate();
            }}
          >
            {check.isPending ? t('common.working') : t('settings.check.run')}
          </button>
          {current.token_configured && (
            <button
              type="button"
              className="button"
              onClick={() => {
                clear.mutate();
              }}
            >
              {t('settings.token.clear')}
            </button>
          )}
        </div>
        <ErrorText error={check.error} />
        {check.data && (
          <ul className="checklist">
            <li className="ok">
              {t('settings.check.reachable', { branch: check.data.default_branch })}
            </li>
            <li className={check.data.can_push ? 'ok' : 'bad'}>{t('settings.check.push')}</li>
            <li className={check.data.branch_found ? 'ok' : 'bad'}>
              {t('settings.check.branch', { branch: form.branch })}
            </li>
            <li className={check.data.workflow_found ? 'ok' : 'bad'}>
              {t('settings.check.workflow', { file: form.workflow_file })}
            </li>
          </ul>
        )}
      </form>
    </div>
  );
}

export function SettingsScreen() {
  const settings = useBuildSettings();
  return (
    <>
      <ErrorText error={settings.error} />
      {settings.data && <SettingsForm current={settings.data} />}
    </>
  );
}
