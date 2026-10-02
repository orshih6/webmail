// Search across all folders: a message filed in a subfolder is found, labelled with its
// folder, opens in place, and actions on it hit the right folder.
import { BASE as base, EXPECTED_NOISE, apiClient, finish, launch, resetAll } from './lib.mjs';

const problems = [];
const step = (s) => console.log('✓', s);
const tag = Date.now() % 100000;
const subject = `Filed away ${tag}`;
const term = `filedaway${tag}`;

await resetAll();
const alice = await apiClient('alice@example.test', 'alicepass');
await alice('POST', '/api/folders/create', { parent: null, name: `Projects ${tag}` });
const folder = (await alice('GET', '/api/folders')).find((f) => f.name === `Projects ${tag}`).path;
await alice('POST', '/api/send', {
  to: 'alice@example.test', cc: '', bcc: '', subject, text: `keyword ${term}`, uploads: [], keep: null,
  in_reply_to: null, references: [], reply_of: null, forward_of: null, draft: null
});
let uid;
for (let i = 0; !uid && i < 60; i++) {
  const page = await alice('GET', `/api/messages?folder=INBOX&q=${term}`);
  uid = page.messages[0]?.uid;
  if (!uid) await new Promise((r) => setTimeout(r, 500));
}
await alice('POST', '/api/messages/move', { folder: 'INBOX', uids: [uid], to: folder });

const browser = await launch();
const p = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
p.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
p.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(m.text()));
await p.goto(base + '/login');
await p.fill('input[type=email]', 'alice@example.test');
await p.fill('input[type=password]', 'alicepass');
await p.click('button[type=submit]');
await p.waitForURL('**/mail?f=INBOX');

await p.fill('input[aria-label=Search]', term);
await p.keyboard.press('Enter');
await p.waitForSelector('text=Nothing matches your search.');
step('search in Inbox: not there (it was filed away)');

await p.check('label.scope input');
// The Sent copy matches too (it is ours), so pick the one in the project folder.
const hit = p.locator('.hits li', { hasText: subject }).filter({ has: p.locator('.in-folder', { hasText: `Projects ${tag}` }) });
await hit.waitFor();
if ((await p.locator('.hits li', { hasText: subject }).count()) !== 2) problems.push('expected the filed copy and the Sent copy');
step('All folders: found in its folder (and the Sent copy), each labelled');

await hit.locator('.row').click();
await p.waitForSelector(`article h2:has-text("${subject}")`);
if (new URL(p.url()).searchParams.get('f') !== folder) problems.push('opened in the wrong folder: ' + p.url());
step('opening a hit shows the message from its own folder');

await p.keyboard.press('#');
await p.waitForSelector('.toast:has-text("moved to Trash")');
await p.locator('.hits li', { hasText: subject }).locator('.in-folder:has-text("Trash")').waitFor();
step('delete from results acts on the right message; results refresh (now in Trash)');

await browser.close();
finish('search all folders', problems);
