import vue from '@vitejs/plugin-vue';
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      // Include every production TypeScript module so unimported files cannot
      // inflate the result. Stateful Vue dialogs are verified through mounted
      // interaction tests; generated render functions are intentionally kept
      // out of this logic-oriented threshold.
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.test.ts', 'src/**/*.d.ts', 'src/components/ui/**', 'src/components/icons/**'],
      reporter: ['text-summary', 'html', 'lcov'],
      reportsDirectory: 'coverage',
      // Rounded baseline floors stop meaningful regressions without coupling CI to exact
      // instrumentation counts. Raise them deliberately as additional production paths gain
      // focused tests.
      thresholds: {
        statements: 75,
        branches: 69,
        functions: 87,
        lines: 77,
      },
    },
  },
});
