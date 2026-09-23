import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { SYNC_ENTITY_STRATEGY } from '../sync';
import { APPEND_ONLY_TABLES, LOCAL_TABLES } from '../tables';

const CONTRACT = fileURLToPath(new URL('../../contracts/db-schema.json', import.meta.url));

function derived() {
  const tables = Object.fromEntries(
    Object.entries(LOCAL_TABLES).map(([name, schema]) => [
      name,
      {
        columns: Object.keys(schema.shape),
        append_only: (APPEND_ONLY_TABLES as readonly string[]).includes(name),
        /** Conflict strategy when synced; `null` = local-only table. */
        sync: (SYNC_ENTITY_STRATEGY as Record<string, string | undefined>)[name] ?? null,
      },
    ]),
  );
  return {
    _comment:
      'Generated from @pos/shared LOCAL_TABLES (POS_REGENERATE_FIXTURES=1 pnpm test). The Rust migrations are tested against this file.',
    tables,
  };
}

describe('db-schema contract', () => {
  it('matches the Zod row schemas', () => {
    if (process.env['POS_REGENERATE_FIXTURES']) {
      writeFileSync(CONTRACT, `${JSON.stringify(derived(), null, 2)}\n`);
    }
    expect(JSON.parse(readFileSync(CONTRACT, 'utf8'))).toEqual(derived());
  });

  it('only syncs tables that exist locally', () => {
    for (const entity of Object.keys(SYNC_ENTITY_STRATEGY)) {
      expect(Object.keys(LOCAL_TABLES)).toContain(entity);
    }
  });

  it('gives every table the soft-delete base columns', () => {
    for (const schema of Object.values(LOCAL_TABLES)) {
      expect(Object.keys(schema.shape)).toEqual(
        expect.arrayContaining(['id', 'created_at', 'updated_at', 'deleted_at']),
      );
    }
  });
});
