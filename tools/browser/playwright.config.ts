import { defineConfig } from '@playwright/test';
import { existsSync } from 'node:fs';

const chrome = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
export default defineConfig({
  testDir: '.',
  testMatch: 'responsive.spec.ts',
  outputDir: '../../target/browser-results',
  workers: 1,
  retries: 0,
  timeout: 60_000,
  use: {
    browserName: 'chromium',
    headless: true,
    deviceScaleFactor: 1,
    launchOptions: existsSync(chrome) ? { executablePath: chrome } : {},
  },
});
