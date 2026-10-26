// vitest.config.ts
// Vitest configuration for unit tests of pure TypeScript logic.
// Tests live in scripts/**/*.test.ts.
// Uses vitest/config defineConfig (not Astro's getViteConfig) since no
// .astro component rendering is required in the test suite.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['scripts/**/*.test.ts'],
    environment: 'node',
  },
});
