// Opened and prefetched messages show instantly from the tab's cache; sign-out clears it.
// Proof, not timing luck: once a message is cached, the network for it is slowed to 3 s —
// if it still appears within 800 ms, it came from the cache.
import { BASE as base, EXPECTED_NOISE, finish, launch, resetAll } from './lib.mjs';

const problems = [];
const step = (s) => console.log('✓', s);
await resetAll();
const browser = await launch();
const p = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
p.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
p.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(m.text()));
const fetched = [];
p.on('request', (r) => {
  const u = new URL(r.url());
  if (u.pathname === '/api/message') fetched.push(u.searchParams.get('uid'));
});
async function signIn() {
  await p.goto(base + '/login');
  await p.fill('input[type=email]', 'alice@example.test');
  await p.fill('input[type=password]', 'alicepass');
  await p.click('button[type=submit]');
  await p.waitForURL('**/mail?f=INBOX');
  await p.waitForSelector('.rows li >> nth=1');
}
const slow = (uid) =>
  p.route(new RegExp(`/api/message\\?.*uid=${uid}(&|$)`), async (route) => {
    await new Promise((r) => setTimeout(r, 3000));
    await route.continue().catch(() => {});
  });
const uidAt = (i) => p.locator('.rows li').nth(i).getAttribute('data-uid');
const subjectAt = async (i) => (await p.locator('.rows li').nth(i).locator('.subject').textContent()).trim();

await signIn();
const [first, second] = [await uidAt(0), await uidAt(1)];
const secondSubject = await subjectAt(1);
await p.locator('.rows li .row').first().click();
await p.waitForSelector('article h2');
for (let i = 0; i < 40 && !fetched.includes(second); i++) await p.waitForTimeout(100);
if (!fetched.includes(second)) problems.push('next message was not prefetched');
step('opening a message prefetches the next one');

await slow(second);
const t0 = Date.now();
await p.keyboard.press('j');
await p.waitForSelector(`article h2:has-text("${secondSubject.replace(/"/g, '\\"')}")`, { timeout: 800 }).catch(() => problems.push('next message did not come from the cache'));
step(`j shows the prefetched message in ${Date.now() - t0} ms (network for it slowed to 3 s)`);

await slow(first);
const t1 = Date.now();
await p.keyboard.press('k');
await p.waitForSelector('article h2', { timeout: 800 });
if (new URL(p.url()).searchParams.get('m') !== first) problems.push('k did not go back');
step(`going back to an opened message is instant too (${Date.now() - t1} ms)`);

// Sign out and in again: nothing may be served from the old cache.
await p.unroute(/.*/);
await p.click('button[aria-label="Sign out"]');
await p.waitForURL('**/login');
await signIn();
await slow(first);
await p.locator(`.rows li[data-uid="${first}"] .row`).click();
await p.waitForTimeout(800);
if (await p.locator('article h2').count()) problems.push('after sign-out a message was still served from the cache');
await p.waitForSelector('article h2', { timeout: 6000 });
step('after sign-out and in again, messages load from the server (cache was cleared)');

await browser.close();
finish('message caching', problems);
