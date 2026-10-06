/** The first-run steps, in the order they have to happen. */
export const STEP_IDS = [
  'licenseKey',
  'github',
  'client',
  'build',
  'download',
  'activate',
  'backup',
] as const;
export type StepId = (typeof STEP_IDS)[number];

/**
 * - `done`: finished.
 * - `next`: the first unfinished step (highlighted).
 * - `waiting`: started, waiting on something outside the app (a running build).
 * - `todo`: later.
 */
export type StepState = 'done' | 'next' | 'waiting' | 'todo';

/** What the generator knows about the setup so far. */
export interface SetupFacts {
  hasLicenseKey: boolean;
  githubConnected: boolean;
  clientCount: number;
  buildRunning: boolean;
  buildReady: boolean;
  downloaded: boolean;
  licensed: boolean;
  keysBackedUp: boolean;
}

export function stepStates(f: SetupFacts): Record<StepId, StepState> {
  const finished: Record<StepId, boolean> = {
    licenseKey: f.hasLicenseKey,
    github: f.githubConnected,
    client: f.clientCount > 0,
    build: f.buildReady,
    download: f.downloaded,
    activate: f.licensed,
    // The update key exists only after the first build.
    backup: f.keysBackedUp,
  };
  const states = {} as Record<StepId, StepState>;
  let nextGiven = false;
  for (const id of STEP_IDS) {
    if (finished[id]) {
      states[id] = 'done';
    } else if (!nextGiven) {
      nextGiven = true;
      states[id] = id === 'build' && f.buildRunning ? 'waiting' : 'next';
    } else {
      states[id] = 'todo';
    }
  }
  return states;
}
