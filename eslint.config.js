// @ts-check
import js from '@eslint/js';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: [
      '**/dist/**',
      '**/node_modules/**',
      '**/.turbo/**',
      'target/**',
      'apps/*/src-tauri/**',
      'eslint.config.js',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  ...tseslint.configs.stylisticTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
      globals: { ...globals.browser, ...globals.node },
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/consistent-type-imports': 'error',
      '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      // The frontend must never talk to SQLite, Supabase or hardware directly.
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/plugin-sql',
              message: 'SQLite is Rust-only. Use a typed IPC command.',
            },
            {
              name: '@supabase/supabase-js',
              message: 'Supabase is Rust-only. Use a typed IPC command.',
            },
          ],
          patterns: [
            {
              group: ['@tauri-apps/api/core'],
              importNames: ['invoke'],
              message: 'Use the typed IPC client (src/ipc) instead of raw invoke().',
            },
          ],
        },
      ],
    },
  },
  {
    files: ['apps/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    rules: reactHooks.configs.recommended.rules,
  },
  {
    files: ['apps/*/src/ipc/**/*.ts'],
    rules: { 'no-restricted-imports': 'off' },
  },
  {
    // Build tooling configs run in Node, outside the app type projects.
    files: ['**/vite.config.ts', '**/vitest.config.ts'],
    ...tseslint.configs.disableTypeChecked,
  },
  {
    files: ['**/*.test.ts', '**/*.test.tsx'],
    rules: {
      '@typescript-eslint/no-non-null-assertion': 'off',
    },
  },
);
