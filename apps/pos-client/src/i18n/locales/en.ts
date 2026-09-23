export const en = {
  app: {
    title: 'Point of Sale',
    loading: 'Starting…',
  },
  shell: {
    ready: 'Shell ready',
    phase: 'Phase 2 · Licensing & secure storage',
    businessType: {
      retail: 'Retail',
      cafe: 'Cafe',
      restaurant: 'Restaurant',
    },
    baseCurrency: 'Base currency',
    version: 'Version {{version}} ({{profile}})',
    language: 'Language',
    notInTauri: 'This screen must run inside the POS desktop app.',
    error: 'The POS core could not be reached: {{message}}',
  },
  license: {
    checking: 'Checking license…',
    devKeyBanner:
      'Development license key — this build accepts licenses anyone can create. Not for real customers.',
    graceBanner_one: 'Offline: connect to the internet within {{count}} day to keep trading.',
    graceBanner_other: 'Offline: connect to the internet within {{count}} days to keep trading.',
    activation: {
      step1: '1 · Send this activation code to your POS provider',
      device: 'This till: {{name}}',
      copy: 'Copy code',
      copied: 'Copied ✓',
      step2: '2 · Paste the license you received',
      tokenPlaceholder: 'eyJhbGciOiJSUzI1NiIs…',
      submit: 'Activate',
      activating: 'Activating…',
    },
    state: {
      missing: {
        title: 'Activate this till',
        body: 'This POS needs a license before it can be used.',
      },
      invalid_token: {
        title: 'License not accepted',
        body: 'The installed license is not valid for this POS.',
      },
      fingerprint_mismatch: {
        title: 'Different computer detected',
        body: 'The license belongs to another computer, or this computer’s hardware changed. Request a new license.',
      },
      expired: { title: 'License expired', body: 'Renew the license with your POS provider.' },
      revoked: {
        title: 'License revoked',
        body: 'This till has been deactivated by your POS provider.',
      },
      grace_exhausted: {
        title: 'Offline for too long',
        body: 'Connect this till to the internet so the license can be confirmed.',
      },
      hardware_error: {
        title: 'Hardware check failed',
        body: 'The computer’s identity could not be read. Restart the till; contact support if it persists.',
      },
      storage_error: {
        title: 'Storage problem',
        body: 'Local data could not be opened. Contact support.',
      },
    },
  },
} as const;
