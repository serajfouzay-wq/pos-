import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Tauri sets these when it drives Vite (`tauri dev` / `tauri build`).
const devHost = process.env['TAURI_DEV_HOST'];
const platform = process.env['TAURI_ENV_PLATFORM'];
const debug = Boolean(process.env['TAURI_ENV_DEBUG']);

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: devHost ?? false,
    ...(devHost ? { hmr: { protocol: 'ws', host: devHost, port: 1431 } } : {}),
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    // WebView2 on Windows is evergreen Chromium; WebKitGTK elsewhere is dev-only.
    target: platform === 'windows' ? 'chrome110' : 'safari16',
    minify: !debug,
    sourcemap: debug,
  },
});
