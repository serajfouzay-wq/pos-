export const en = {
  app: {
    title: 'POS Factory',
    loading: 'Starting…',
  },
  nav: {
    clients: 'Clients',
    builds: 'Builds',
    licenses: 'Licenses',
    settings: 'Settings',
  },
  shell: {
    phase: 'Phase 2 · Licensing & secure storage',
    comingSoon: 'This workspace arrives in Phase 5.',
    version: 'Version {{version}} ({{profile}})',
    language: 'Language',
    notInTauri: 'This screen must run inside the POS Factory desktop app.',
    error: 'The generator core could not be reached: {{message}}',
  },
  licenses: {
    copy: 'Copy',
    copied: 'Copied ✓',
    working: 'Working…',
    businessType: { retail: 'Retail', cafe: 'Cafe', restaurant: 'Restaurant' },
    key: {
      title: 'Signing key',
      state: {
        absent: 'No signing key yet. Create one to start issuing licenses.',
        locked: 'The signing key is locked. Enter its passphrase to issue licenses.',
        unlocked: 'Unlocked for this session. Lock it when you are done.',
      },
      backupWarning:
        'Back up the key file and remember the passphrase. If either is lost, every client build must be recompiled with a new key.',
      passphrase: 'Passphrase',
      confirm: 'Confirm passphrase',
      minLength: 'At least {{count}} characters.',
      create: 'Create signing key',
      unlock: 'Unlock',
      lock: 'Lock key',
      keyId: 'Key id:',
      publicKeyHelp:
        'Public key — build clients with POS_LICENSE_PUBLIC_KEY set to this, and store it as the LICENSE_PUBLIC_KEY_PEM secret of the license-validate function.',
    },
    issue: {
      title: 'Issue a device license',
      unlockFirst: 'Unlock the signing key first.',
      code: 'Activation code from the till',
      device: 'Device',
      client: 'Client id',
      fingerprint: 'Fingerprint',
      slug: 'Client slug',
      businessType: 'Business type',
      maxDevices: 'Max devices (whole client)',
      expiry: 'Expires (optional)',
      submit: 'Sign license',
      result: 'Send this license back to the till and paste it into its activation screen.',
    },
  },
} as const;
