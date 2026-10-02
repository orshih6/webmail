// Delete, junk and drag-move offer Undo; permanent delete (from Trash) asks first.
import { BASE as base, EXPECTED_NOISE, apiClient, finish, launch, resetAll } from './lib.mjs';

const problems = [];
const step = (s) => console.log('✓', s);
const tag = Date.now() % 100000;

await resetAll();
// A message of our own to throw around.
const subject = `Undo me ${tag}`;
const alice = await apiClient('alice@example.test', 'alicepass');
await alice('POST', '/api/send', {
  to: 'alice@example.test', cc: '', bcc: '', subject, text: 'x', uploads: [], keep: null,
  in_reply_to: null, references: [], reply_of: null, forward_of: null, draft: null
});

const browser = await launch();
const p = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
p.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
p.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(m.text()));
await p.goto(base + '/login');
await p.fill('input[type=email]', 'alice@example.test');
await p.fill('input[type=password]', 'alicepass');
await p.click('button[type=submit]');
await p.waitForURL('**/mail?f=INBOX');
const row = () => p.locator('.rows li', { hasText: subject });
await row().waitFor({ timeout: 30000 });

// Delete with the keyboard, then Undo.
await row().locator('.row').click();
await p.keyboard.press('#');
await row().waitFor({ state: 'detached' });
await p.click('.toast:has-text("moved to Trash") button:has-text("Undo")');
await p.waitForSelector('text=Undone');
await row().waitFor();
step('delete → Undo puts it back in Inbox');

// Drag onto Junk, then Undo.
await row().dragTo(p.locator('nav li a', { hasText: 'Junk' }));
await row().waitFor({ state: 'detached' });
await p.click('.toast:has-text("Moved message to Junk") button:has-text("Undo")');
await row().waitFor();
step('drag to Junk → Undo puts it back');

// From Trash, delete is permanent: confirm first. Cancelling keeps the message.
await row().locator('.row').click();
await p.keyboard.press('#');
await p.click('nav a:has-text("Trash")');
await row().locator('.row').click();
let asked = '';
p.once('dialog', (d) => { asked = d.message(); d.dismiss(); });
await p.keyboard.press('#');
await p.waitForTimeout(500);
if (!/Permanently delete/.test(asked)) problems.push('no confirmation for permanent delete');
if (!(await row().count())) problems.push('cancelled permanent delete still removed the message');
p.once('dialog', (d) => d.accept());
await p.keyboard.press('#');
await row().waitFor({ state: 'detached' });
await p.waitForSelector('.toast:has-text("deleted")');
if (await p.locator('.toast:has-text("deleted") button:has-text("Undo")').count()) problems.push('permanent delete offered Undo');
step('permanent delete from Trash asks first; no Undo offered');

await browser.close();
finish('undo', problems);
