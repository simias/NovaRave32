import { defineConfig } from 'vite';
import checker from 'vite-plugin-checker';
import eslint from 'vite-plugin-eslint';

export default defineConfig({
  plugins: [eslint(), checker({ typescript: true })],
});
