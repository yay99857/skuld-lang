import { defineConfig } from '@playwright/test';

// The browser tests run against the built site, served the way `npm run dev`
// serves it. Locally an already running server is reused; in CI one is
// started for the run and stopped after it.
export default defineConfig({
  testDir: './tests',
  use: { baseURL: 'http://127.0.0.1:4173', browserName: 'chromium' },
  reporter: 'list',
  webServer: {
    command: 'npm run dev',
    url: 'http://127.0.0.1:4173',
    reuseExistingServer: !process.env.CI,
  },
});
