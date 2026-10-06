import { describe, expect, it } from 'vitest';
import { stepStates, type SetupFacts } from './steps';

const nothing: SetupFacts = {
  hasLicenseKey: false,
  githubConnected: false,
  clientCount: 0,
  buildRunning: false,
  buildReady: false,
  downloaded: false,
  licensed: false,
  keysBackedUp: false,
};

describe('first-run steps', () => {
  it('starts with the license key and points at one next step', () => {
    const states = stepStates(nothing);
    expect(states.licenseKey).toBe('next');
    expect(Object.values(states).filter((s) => s === 'next')).toHaveLength(1);
    expect(states.backup).toBe('todo');
  });

  it('moves on as things get done, and shows a running build as waiting', () => {
    const states = stepStates({
      ...nothing,
      hasLicenseKey: true,
      githubConnected: true,
      clientCount: 1,
      buildRunning: true,
    });
    expect(states.client).toBe('done');
    expect(states.build).toBe('waiting');
    expect(states.download).toBe('todo');
  });

  it('a step done out of order still counts', () => {
    const states = stepStates({ ...nothing, keysBackedUp: true });
    expect(states.backup).toBe('done');
    expect(states.licenseKey).toBe('next');
  });
});
