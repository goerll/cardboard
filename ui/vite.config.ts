import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The Rust server serves dist/; `pnpm dev` proxies the WebSocket to `cardboard serve --port 4610`.
export default defineConfig({
  plugins: [react()],
  base: '/',
  build: { outDir: 'dist', emptyOutDir: true, target: 'chrome120', sourcemap: true },
  server: { port: 5173, proxy: { '/ws': { target: 'ws://127.0.0.1:4610', ws: true } } },
});
