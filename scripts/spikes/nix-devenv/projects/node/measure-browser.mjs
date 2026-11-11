import { chromium } from 'playwright';

const started = performance.now();
const browser = await chromium.launch({ args: ['--no-sandbox'] });
const launched = performance.now();

const page = await browser.newPage();
const pageOpened = performance.now();

await page.setContent(`
  <!doctype html>
  <html>
    <head><title>Factory Browser Timing</title></head>
    <body><h1 id="result">pending</h1></body>
  </html>
`);
await page.evaluate(() => {
  document.querySelector('#result').textContent = 'ok';
});
await page.waitForSelector('#result', { state: 'attached' });
const rendered = performance.now();

console.log(
  `BROWSER_TIMING ${JSON.stringify({
    launchMs: Math.round(launched - started),
    newPageMs: Math.round(pageOpened - launched),
    renderMs: Math.round(rendered - pageOpened),
  })}`,
);

await browser.close();
