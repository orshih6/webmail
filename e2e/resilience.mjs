// The mail server freezes mid-session, then comes back; the browser goes offline, then
// back online. The user should see one calm banner (no error toasts) and the app should
// recover by itself. Freezes the dev stack's mail container with `docker compose pause`.
import { execFileSync } from 'node:child_process';
import { BASE as base, EXPECTED_NOISE, SHOTS, finish, launch, resetAll } from './lib.mjs';

const compose = new URL('../dev/compose.yml', import.meta.url).pathname;
const dc = (...args) => execFileSync('docker', ['compose', '-f', compose, ...args], { stdio: 'pipe' });
const problems = [];
const step = (s) => console.log('✓', s);
// Expected here on top of the usual: 503s while the mail server is frozen…
// …and requests that happen to be in flight while the test takes the browser offline.
const noise = (t) => EXPECTED_NOISE.test(t) || /503|ERR_INTERNET_DISCONNECTED/.test(t);

await resetAll();
const browser = await launch();
const ctx = await browser.newContext({ viewport: { width: 1400, height: 860 } });
const p = await ctx.newPage();
p.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
p.on('console', (m) => m.type() === 'error' && !noise(m.text()) && problems.push(m.text()));
await p.goto(base + '/login');
await p.fill('input[type=email]', 'alice@example.test');
await p.fill('input[type=password]', 'alicepass');
await p.click('button[type=submit]');
await p.waitForURL('**/mail?f=INBOX');
await p.waitForSelector('.rows li');

try {
  dc('pause', 'mail');
  await p.click('button[title=Refresh]');
  await p.waitForSelector('text=Can\'t reach the mail server', { timeout: 20000 });
  await p.screenshot({ path: SHOTS + 'r-mail-down.png' });
  if (await p.locator('.toast.error').count()) problems.push('error toast shown alongside the banner');
  // Navigating while down keeps the banner and adds no toasts.
  await p.click('nav a:has-text("Sent")');
  await p.waitForTimeout(1500);
  if (await p.locator('.toast.error').count()) problems.push('error toast while navigating during outage');
  step('mail server frozen: banner shown, no error toasts');
} finally {
  dc('unpause', 'mail');
}
await p.waitForSelector('text=Can\'t reach the mail server', { state: 'detached', timeout: 60000 });
await p.waitForSelector('.rows li', { timeout: 20000 });
await p.locator('.rows li .row').first().click();
await p.waitForSelector('article h2');
step('mail server back: banner clears on its own, mail loads and opens');

await ctx.setOffline(true);
await p.waitForSelector('text=You\'re offline');
await ctx.setOffline(false);
await p.waitForSelector('text=You\'re offline', { state: 'detached' });
step('browser offline → banner; online again → cleared');

await browser.close();
finish('resilience', problems);
