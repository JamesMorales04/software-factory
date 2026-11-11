import { test, expect } from '@playwright/test';

test('Chromium renders a page and runs JavaScript', async ({ page }) => {
  await page.setContent(`
    <!doctype html>
    <html>
      <head><title>Factory Playwright Spike</title></head>
      <body><h1 id="result">pending</h1></body>
    </html>
  `);

  await page.evaluate(() => {
    document.querySelector('#result').textContent = 'ok';
  });

  await expect(page).toHaveTitle('Factory Playwright Spike');
  await expect(page.locator('#result')).toHaveText('ok');
});
