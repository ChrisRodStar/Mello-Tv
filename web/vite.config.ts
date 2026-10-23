import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Development only. Production serves dist through the Go API and routes
// /stream directly to the relay. These variables are never bundled into clients.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': {
        target: process.env.FAMILY_STREAM_API_ORIGIN ?? 'http://127.0.0.1:8090',
        changeOrigin: false,
      },
      '/stream': {
        target: process.env.FAMILY_STREAM_STREAM_ORIGIN ?? 'http://127.0.0.1:8095',
        changeOrigin: false,
        configure(proxy) {
          // Vite's default proxy-error handler prints res.req.url; redact before
          // its listener runs so an unreachable relay never logs a capability.
          proxy.on('error', (_error, req, res) => {
            req.url = '/stream/redacted';
            if ('req' in res) res.req.url = '/stream/redacted';
          });
        },
      },
    },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/react/') || id.includes('node_modules/react-dom/'))
            return 'vendor-react';
        },
      },
    },
  },
});
