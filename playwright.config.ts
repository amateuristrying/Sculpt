import { defineConfig } from '@playwright/test'
export default defineConfig({
  testDir: './tests/ui', workers: 1, timeout: 120_000,
  use: { baseURL: 'http://127.0.0.1:1420', channel: 'chrome', viewport: { width: 1440, height: 1000 },
    launchOptions: { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] },
    screenshot: 'only-on-failure', trace: 'retain-on-failure' },
  webServer: { command: 'npm run dev', url: 'http://127.0.0.1:1420', reuseExistingServer: !process.env.CI },
})
