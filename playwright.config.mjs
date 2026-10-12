import {defineConfig} from '@playwright/test';

export default defineConfig({
  testDir: './tests/ui',
  testMatch: '**/*.spec.mjs',
  timeout: 180000,
  expect: {timeout: 15000},
  workers: 1,
  retries: 0,
  forbidOnly: Boolean(process.env.CI),
  reporter: process.env.CI ? [['line'], ['html', {open: 'never'}]] : 'list',
  use: {
    baseURL: 'http://127.0.0.1:5180',
    viewport: {width: 1440, height: 1000},
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  projects: [{name: 'chromium', use: {browserName: 'chromium'}}],
  webServer: {
    command: 'node ui/serve.mjs',
    env: {PORT: '5180'},
    url: 'http://127.0.0.1:5180/tests/ui/index.html',
    reuseExistingServer: false,
  },
});
