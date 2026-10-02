// The session expires while a message is being written; a tab closes before the draft is
// saved. Nothing the user wrote should be lost.
import { execFileSync } from 'node:child_process';
import { BASE as base, EXPECTED_NOISE, apiClient, finish, launch, resetAll } from './lib.mjs';

const compose = new URL('../dev/compose.yml', import.meta.url).pathname;
const expireSessions = (email) =>
  execFileSync('docker', ['compose', '-f', compose, 'exec', '-T', 'postgres', 'psql', '-U', 'postgres', '-d', 'webmail', '-qc',
    `DELETE FROM sessions WHERE email = '${email}'`]);
const problems = [];
const step = (s) => console.log('✓', s);
const tag = Date.now() % 100000;

await resetAll();
const browser = await launch();
const ctx = await browser.newContext({ viewport: { width: 1400, height: 860 } });
let p = await ctx.newPage();
const watch = (pg) => {
  pg.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  pg.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(m.text()));
};
watch(p);
await p.goto(base + '/login');
await p.fill('input[type=email]', 'alice@example.test');
await p.fill('input[type=password]', 'alicepass');
await p.click('button[type=submit]');
await p.waitForURL('**/mail?f=INBOX');

// ---- Session expires mid-compose: sign in over the page, the send goes through ----
const subject = `Survives expiry ${tag}`;
await p.click('a:has-text("Compose")');
await p.locator('input[aria-label=To]').fill('bob@example.test');
await p.fill('label:has-text("Subject") input', subject);
await p.fill('textarea', 'Written before the session ran out.');
expireSessions('alice@example.test');
await p.click('button:has-text("Send")');
await p.waitForSelector('dialog[open] >> text=Your session expired');
await p.fill('dialog input[type=password]', 'wrong');
await p.click('dialog button:has-text("Continue")');
await p.waitForSelector('dialog .error');
await p.fill('dialog input[type=password]', 'alicepass');
await p.click('dialog button:has-text("Continue")');
await p.waitForSelector('text=Message sent', { timeout: 20000 });
const bob = await apiClient('bob@example.test', 'bobpass');
for (let i = 0; ; i++) {
  const page = await bob('GET', `/api/messages?folder=INBOX&q=${encodeURIComponent(subject)}`);
  if (page.messages.some((m) => m.subject === subject)) break;
  if (i > 40) { problems.push('message sent after re-login never arrived'); break; }
  await new Promise((r) => setTimeout(r, 500));
}
step('session expired mid-compose: wrong password rejected, right one resumes the send, it arrives');

// ---- Tab closes before the draft is saved: offered back next time ----
const lost = `Unsaved ${tag}`;
await p.click('a:has-text("Compose")');
await p.fill('label:has-text("Subject") input', lost);
await p.fill('textarea', 'This tab is about to close.');
await p.waitForTimeout(1500); // backup is debounced
await p.close({ runBeforeUnload: false });
p = await ctx.newPage();
watch(p);
await p.goto(base + '/mail/compose');
await p.waitForSelector(`text=You have an unsent message “${lost}”`);
await p.click('button:has-text("Restore it")');
if ((await p.inputValue('label:has-text("Subject") input')) !== lost) problems.push('subject not restored');
if (!(await p.inputValue('textarea')).includes('about to close')) problems.push('body not restored');
step('closed tab: the unsent message is offered back and restored');

// ---- Signing out leaves nothing behind in this browser ----
await p.waitForTimeout(1200); // restored text is backed up again
await p.click('button[aria-label="Sign out"]');
await p.waitForURL('**/login');
const left = await p.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith('compose-backup:')));
if (left.length) problems.push('backup survived sign-out: ' + left);
step('sign-out clears unsent-message backups');

await browser.close();
finish('session expiry & unsaved work', problems);
