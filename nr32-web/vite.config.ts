import { defineConfig } from 'vite';
import checker from 'vite-plugin-checker';
import eslint from 'vite-plugin-eslint';

export default defineConfig({
  plugins: [eslint(), checker({ typescript: true })],
  base: '/nr32/',
  server: {
    headers: {
      // Required for SharedArrayBuffer using the Vite dev server
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
});
