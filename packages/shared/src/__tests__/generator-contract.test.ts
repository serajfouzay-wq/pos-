import { describe, expect, it } from 'vitest';
import fixture from '../../contracts/generator-examples.json';
import configExample from '../../contracts/client-config.example.json';
import { ClientConfigSchema } from '../client-config';
import { GENERATOR_IPC } from '../ipc/generator-contract';
import {
  BuildRecordSchema,
  BuildSettingsSchema,
  ClientDetailSchema,
  ClientSummarySchema,
  IssuedLicenseRecordSchema,
  NewClientInputSchema,
  ReceiptPreviewSchema,
  RepoCheckSchema,
  isActiveBuild,
} from '../ipc/generator-types';

/** Every example Rust emits must satisfy the schema the UI validates with. */
const cases = {
  client_detail: ClientDetailSchema,
  client_summary: ClientSummarySchema,
  issued_license: IssuedLicenseRecordSchema,
  build_record: BuildRecordSchema,
  build_settings: BuildSettingsSchema,
  repo_check: RepoCheckSchema,
  receipt_preview: ReceiptPreviewSchema,
} as const;

describe('generator response contract (Rust → Zod)', () => {
  it('covers every example in the fixture', () => {
    expect(Object.keys(fixture.examples).sort()).toEqual(Object.keys(cases).sort());
  });

  it.each(Object.entries(cases))('%s parses', (name, schema) => {
    const example = (fixture.examples as Record<string, unknown>)[name];
    const result = schema.safeParse(example);
    expect(result.error?.issues ?? []).toEqual([]);
  });

  it('the preview is the real receipt layout', () => {
    const preview = ReceiptPreviewSchema.parse(fixture.examples.receipt_preview);
    expect(preview.columns).toBe(48);
    expect(preview.text).toContain('TOTAL KWD');
    expect(preview.logo_png_base64).not.toBeNull();
  });
});

describe('generator inputs', () => {
  it('validates a new client', () => {
    expect(
      NewClientInputSchema.safeParse({
        display_name: 'Acme',
        client_slug: 'acme-cafe',
        business_type: 'cafe',
        base_currency: 'KWD',
      }).success,
    ).toBe(true);
    expect(
      NewClientInputSchema.safeParse({
        display_name: 'Acme',
        client_slug: 'Acme Cafe',
        business_type: 'cafe',
        base_currency: 'KWD',
      }).success,
    ).toBe(false);
  });

  it('licenses are issued for a client, not a free-typed slug', () => {
    const args = GENERATOR_IPC.issue_license.args.shape.request;
    expect(Object.keys(args.shape).sort()).toEqual(
      ['activation_code', 'client_id', 'expires_at', 'max_devices'].sort(),
    );
  });

  it('knows which builds are still running', () => {
    expect(isActiveBuild('queued')).toBe(true);
    expect(isActiveBuild('succeeded')).toBe(false);
  });
});

describe('client config cloud URL', () => {
  const withUrl = (url: string) => ({
    ...configExample,
    cloud: { supabase_url: url, supabase_anon_key: 'anon' },
  });

  it('allows https anywhere and plain http only on loopback (same rule as Rust)', () => {
    expect(ClientConfigSchema.safeParse(withUrl('https://abc.supabase.co')).success).toBe(true);
    expect(ClientConfigSchema.safeParse(withUrl('http://127.0.0.1:54321')).success).toBe(true);
    expect(ClientConfigSchema.safeParse(withUrl('http://localhost:54321')).success).toBe(true);
    expect(ClientConfigSchema.safeParse(withUrl('http://abc.supabase.co')).success).toBe(false);
    expect(ClientConfigSchema.safeParse(withUrl('http://localhost.evil.com')).success).toBe(false);
  });
});
