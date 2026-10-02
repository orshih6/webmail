import { BASE as base, EXPECTED_NOISE, FIXTURES, SHOTS, finish, launch, resetAll } from './lib.mjs';
const shots = SHOTS;
const tag = Date.now() % 100000;
const problems = [];

await resetAll();
const browser = await launch();
async function session(name, viewport = { width: 1400, height: 860 }) {
  const ctx = await browser.newContext({ viewport });
  const page = await ctx.newPage();
  page.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(`[${name}] console: ${m.text()}`));
  page.on('pageerror', (e) => problems.push(`[${name}] pageerror: ${e.message}`));
  return page;
}
async function login(page, email, pass) {
  await page.goto(base + '/');
  await page.waitForURL('**/login');
  await page.fill('input[type=email]', email);
  await page.fill('input[type=password]', pass);
  await page.click('button[type=submit]');
  await page.waitForURL('**/mail?f=INBOX');
  await page.waitForSelector('nav li a');
}
const step = (s) => console.log('✓', s);

const alice = await session('alice');
await alice.goto(base + '/login');
await alice.screenshot({ path: shots + '01-login.png' });
await alice.fill('input[type=email]', 'alice@example.test');
await alice.fill('input[type=password]', 'wrong');
await alice.click('button[type=submit]');
await alice.waitForSelector('.error');
step('wrong password shows: ' + (await alice.textContent('.error')));
await login(alice, 'alice@example.test', 'alicepass');
step('alice signed in');

// HTML newsletter: sanitized, sandboxed, remote content blocked until asked.
await alice.click('text=October product update');
const frame = alice.frameLocator('iframe[title="Message body"]');
await frame.locator('h1').waitFor();
const html = await alice.getAttribute('iframe[title="Message body"]', 'srcdoc');
if (/script|tracker\.example|javascript:/.test(html)) problems.push('unsafe content in srcdoc');
if ((await alice.title()).includes('pwned')) problems.push('script ran!');
await alice.waitForSelector('text=Remote images are hidden');
step('newsletter rendered; script + tracker stripped; banner shown');
await alice.screenshot({ path: shots + '02-reading-html.png' });
await alice.click('text=Show images');
await alice.waitForFunction(() => document.querySelector('iframe')?.getAttribute('srcdoc')?.includes('tracker.example'));
step('show images re-renders with remote images');

// Compose to bob.
const bob = await session('bob');
await login(bob, 'bob@example.test', 'bobpass');
const subject = `Lunch on Friday? ${tag}`;
await alice.click('a:has-text("Compose")');
await alice.waitForURL('**/mail/compose');
await alice.fill('label:has-text("To") input', 'Bob <bob@example.test>');
await alice.fill('label:has-text("Subject") input', subject);
await alice.fill('textarea', 'Hi Bob,\n\nAre you free for lunch on Friday?\n\nAlice');
await alice.setInputFiles('input[type=file]', { name: 'menu.txt', mimeType: 'text/plain', buffer: Buffer.from('soup, salad') });
await alice.waitForSelector('.files li:has-text("menu.txt")');
await alice.screenshot({ path: shots + '03-compose.png' });
await alice.keyboard.press('Control+Enter');
await alice.waitForSelector('text=Message sent');
step('alice sent with attachment');

// Bob gets it live (no reload) via SSE.
await bob.waitForSelector(`text=${subject}`, { timeout: 30000 });
step('bob received it live without reloading');
await bob.click(`text=${subject}`);
await bob.waitForSelector('.attachments a:has-text("menu.txt")');
await bob.screenshot({ path: shots + '04-bob-reading.png' });
await bob.keyboard.press('r');
await bob.waitForURL('**/mail/compose?mode=reply**');
await bob.waitForFunction(() => document.querySelector('textarea')?.value.includes('> Are you free'));
await bob.keyboard.type('Sounds good!');
await bob.screenshot({ path: shots + '05-reply.png' });
await bob.click('button:has-text("Send")');
await bob.waitForSelector('text=Message sent');
step('bob replied with quote via keyboard shortcut');

await alice.click('nav a:has-text("Inbox")');
await alice.waitForSelector(`text=Re: ${subject}`, { timeout: 30000 });
await alice.screenshot({ path: shots + '06-alice-inbox-thread.png' });
step('reply arrived and groups with conversation');

// Star + delete with optimistic UI.
await alice.click(`text=Re: ${subject}`);
await alice.keyboard.press('s');
await alice.waitForSelector('.reader .star.on');
await alice.keyboard.press('#');
await alice.waitForSelector(`.rows >> text=Re: ${subject}`, { state: 'detached' });
step('star (s) and delete (#) via keyboard');

// Draft autosave on leaving compose.
await alice.click('a:has-text("Compose")');
await alice.fill('label:has-text("Subject") input', `Unfinished ${tag}`);
await alice.fill('textarea', 'half a thought');
await alice.click('nav a:has-text("Drafts")');
await alice.waitForSelector('text=Saved to Drafts');
await alice.waitForSelector(`text=Unfinished ${tag}`);
await alice.click(`text=Unfinished ${tag}`);
await alice.waitForFunction(() => document.querySelector('textarea')?.value === 'half a thought');
step('leaving compose saves a draft; reopening restores it');
await alice.click('button[aria-label=Discard]').catch(() => {});

// Mobile layout.
const phone = await session('phone', { width: 390, height: 800 });
await login(phone, 'alice@example.test', 'alicepass');
await phone.screenshot({ path: shots + '07-mobile-list.png' });
await phone.click('button[aria-label=Folders]');
await phone.waitForTimeout(300);
await phone.screenshot({ path: shots + '08-mobile-drawer.png' });
await phone.mouse.click(370, 400); // tap outside the drawer
await phone.click('text=October product update');
await phone.frameLocator('iframe').locator('h1').waitFor();
await phone.screenshot({ path: shots + '09-mobile-reading.png' });
step('mobile: list, drawer, reading');

// Dark mode.
const dark = await browser.newContext({ viewport: { width: 1400, height: 860 }, colorScheme: 'dark' });
const dp = await dark.newPage();
await login(dp, 'alice@example.test', 'alicepass');
await dp.click('text=October product update');
await dp.frameLocator('iframe').locator('h1').waitFor();
await dp.screenshot({ path: shots + '10-dark.png' });
step('dark mode');

await browser.close();
finish('core mail', problems);
