import { BASE as base, EXPECTED_NOISE, FIXTURES, SHOTS, apiClient, finish, launch, resetAll } from './lib.mjs';
const tag = Date.now() % 100000;
const problems = [];
const step = (s) => console.log('✓', s);
await resetAll();
// A default identity with a signature, so the rich-text check can verify where typing lands.
await (await apiClient('alice@example.test', 'alicepass'))('POST', '/api/identities', {
  id: null, name: 'Alice', reply_to: '', signature: 'The Support Team', is_default: true
});
const browser = await launch();
async function login(email, pass) {
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 860 }, acceptDownloads: true });
  const p = await ctx.newPage();
  p.on('dialog', (d) => d.accept());
  p.on('pageerror', (e) => problems.push(`${email} pageerror: ${e.message}`));
  p.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(`${email}: ${m.text()}`));
  await p.goto(base + '/login');
  await p.fill('input[type=email]', email);
  await p.fill('input[type=password]', pass);
  await p.click('button[type=submit]');
  await p.waitForURL('**/mail?f=INBOX');
  await p.waitForSelector('nav li a');
  // Start from default prefs.
  await p.evaluate(async () => {
    const { csrf } = await (await fetch('/api/session')).json();
    await fetch('/api/prefs', { method: 'POST', headers: { 'content-type': 'application/json', 'x-csrf-token': csrf }, body: '{}' });
  });
  await p.reload(); await p.waitForSelector('nav li a');
  return p;
}
const folderLink = (p, name) => p.locator('nav li a', { hasText: name });
async function folderMenu(p, name, item) {
  const li = p.locator('nav li', { has: p.locator('a .name', { hasText: new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`) }) });
  await li.hover();
  await li.locator('button[aria-haspopup=menu]').click();
  await p.click(`[role=menuitem]:has-text("${item}")`);
}

const a = await login('alice@example.test', 'alicepass');
// Remove test folders an aborted earlier run left behind (deepest first).
await a.evaluate(async () => {
  const { csrf } = await (await fetch('/api/session')).json();
  const h = { 'content-type': 'application/json', 'x-csrf-token': csrf };
  const fs = (await (await fetch('/api/folders')).json()).filter((f) => !f.special && f.path !== 'INBOX');
  fs.sort((x, y) => y.path.length - x.path.length);
  for (const f of fs) await fetch('/api/folders/delete', { method: 'POST', headers: h, body: JSON.stringify({ folder: f.path }) });
});
await a.reload(); await a.waitForSelector('nav li a');
const b = await login('bob@example.test', 'bobpass');

// ---- Folders ----
const top = `Projects ${tag} ✓`;
await a.click('button[aria-label="New folder"]');
await a.keyboard.type(top); await a.keyboard.press('Enter');
await folderLink(a, top).waitFor();
step('create top-level folder (non-ASCII)');
await folderMenu(a, top, 'New subfolder');
await a.keyboard.type('Q4'); await a.keyboard.press('Enter');
await folderLink(a, 'Q4').waitFor();
await folderMenu(a, 'Q4', 'Rename');
await a.keyboard.type('Q4 2026'); await a.keyboard.press('Enter'); // the field starts selected
await folderLink(a, 'Q4 2026').waitFor();
step('nested subfolder created and renamed inline');
await a.screenshot({ path: SHOTS + 'f-folders.png' });

// Drag a message onto the new folder.
const row = a.locator('.rows li').first();
const subj = (await row.locator('.subject').textContent()).trim();
await row.locator('.row').dragTo(folderLink(a, top));
await a.waitForSelector('text=Moved message to');
await folderLink(a, top).click();
await a.locator('.rows li', { hasText: subj }).first().waitFor();
step('drag a message onto a folder moves it');

// Mark unread → count → "Mark all as read".
await a.locator('.rows li .row').first().click();
await a.keyboard.press('u');
await a.waitForFunction((t) => [...document.querySelectorAll('nav li a')].some((x) => x.textContent.includes(t) && x.querySelector('.count')), top);
await folderMenu(a, top, 'Mark all as read');
await a.waitForFunction((t) => [...document.querySelectorAll('nav li a')].some((x) => x.textContent.includes(t) && !x.querySelector('.count')), top);
step('mark all as read');

// System folders offer no rename/delete.
const inboxLi = a.locator('nav li', { has: a.locator('a', { hasText: 'Inbox' }) });
await inboxLi.hover();
if (await inboxLi.locator('button[aria-haspopup=menu]').count()) {
  await inboxLi.locator('button[aria-haspopup=menu]').click();
  const labels = await a.locator('[role=menuitem]').allTextContents();
  if (labels.some((l) => /Rename|Delete/.test(l))) problems.push('Inbox menu offers ' + labels);
  await a.keyboard.press('Escape');
}
// Move the message back, delete the folders.
await a.locator('.rows li .row').first().dragTo(folderLink(a, 'Inbox'));
await a.waitForSelector('text=Moved message to Inbox');
await folderMenu(a, 'Q4 2026', 'Delete folder');
await folderLink(a, 'Q4 2026').waitFor({ state: 'detached' });
await folderMenu(a, top, 'Delete folder');
await folderLink(a, top).waitFor({ state: 'detached' });
step('system folders protected; user folders deleted (child first)');

// ---- Rich compose with inline image + an image attachment ----
await a.click('a:has-text("Compose")');
await a.click('button:has-text("Rich text")');
await a.locator('input[aria-label=To]').fill('bob@example.test');
const rsubj = `Rich ${tag}`;
await a.fill('label:has-text("Subject") input', rsubj);
const area = a.locator('.area');
await area.focus();
await a.click('button[aria-label^="Bold"]');
await a.keyboard.type('Bold hello');
await a.click('button[aria-label^="Bold"]');
await a.keyboard.press('Enter');
await a.click('button[aria-label="Bulleted list"]');
await a.keyboard.type('one'); await a.keyboard.press('Enter'); await a.keyboard.type('two');
await a.setInputFiles('.toolbar input[type=file]', FIXTURES + 'pixel.png');
await a.waitForSelector('.area img[data-upload]');
await a.setInputFiles('.actions input[type=file]', FIXTURES + 'pixel.png');
await a.waitForSelector('.files li:has-text("pixel.png")');
await a.screenshot({ path: SHOTS + 'f-rich-compose.png' });
await a.keyboard.press('Control+Enter');
await a.waitForSelector('text=Message sent');
step('rich-text message with formatting, inline image and attachment sent');

await b.waitForSelector(`text=${rsubj}`, { timeout: 30000 });
await b.click(`text=${rsubj}`);
const fr = b.frameLocator('iframe[title="Message body"]');
await fr.locator('b, strong').first().waitFor();
const inner = await b.getAttribute('iframe[title="Message body"]', 'srcdoc');
if (!/<li>one<\/li>/.test(inner)) problems.push('list missing');
if (!(inner.indexOf('Bold hello') >= 0 && inner.indexOf('Bold hello') < inner.indexOf('The Support Team')))
  problems.push('typed text should come before the signature');
if (!/data:image\/png;base64/.test(inner)) problems.push('inline image missing');
const prev = b.locator('.previews img');
await prev.waitFor();
await b.waitForFunction(() => document.querySelector('.previews img')?.naturalWidth > 0);
step('bob sees formatting, inline image, and an image attachment preview');
await b.screenshot({ path: SHOTS + 'f-reader-rich.png' });

// ---- Reader menu: source / print / download ----
const [src] = await Promise.all([b.context().waitForEvent('page'), (async () => { await b.click('button[aria-label="More actions"]'); await b.click('[role=menuitem]:has-text("View source")'); })()]);
await src.waitForLoadState();
const srcText = await src.textContent('body');
if (!srcText.includes(`Subject: ${rsubj}`) || !srcText.includes('multipart/related')) problems.push('source view wrong');
await src.close();
const [pr] = await Promise.all([b.context().waitForEvent('page'), (async () => { await b.click('button[aria-label="More actions"]'); await b.click('[role=menuitem]:has-text("Print")'); })()]);
await pr.waitForLoadState();
if (!(await pr.textContent('h1')).includes(rsubj)) problems.push('print view wrong');
await pr.screenshot({ path: SHOTS + 'f-print.png' });
await pr.close();
const [dl] = await Promise.all([b.waitForEvent('download'), (async () => { await b.click('button[aria-label="More actions"]'); await b.click('[role=menuitem]:has-text("Download")'); })()]);
if (!dl.suggestedFilename().endsWith('.eml')) problems.push('download name ' + dl.suggestedFilename());
step(`view source, print, download (${dl.suggestedFilename()})`);

// ---- Filters ----
await b.click('nav a:has-text("Inbox")');
await b.locator('.rows li .row', { hasText: rsubj }).click();
await b.keyboard.press('s');
await b.click('.filters button:has-text("Starred")');
await b.waitForFunction((t) => { const r = [...document.querySelectorAll('.rows li')]; return r.length >= 1 && r.every((x) => x.querySelector('.star.on')) && r.some((x) => x.textContent.includes(t)); }, rsubj);
await b.click('.filters button:has-text("Unread")');
await b.waitForFunction(() => [...document.querySelectorAll('.rows li:not(.empty)')].every((x) => x.classList.contains('unread')));
step('starred and unread filters');
await b.click('.filters button:has-text("All")');

// ---- Rich by default + HTML draft round-trip ----
await a.click('nav a:has-text("Settings")');
await a.click('label:has-text("Write new messages in rich text")');
await a.waitForTimeout(300);
await a.click('a:has-text("Compose")');
await a.waitForSelector('.area');
await a.fill('label:has-text("Subject") input', `Rich draft ${tag}`);
await a.locator('.area').click();
await a.click('button[aria-label^="Italic"]');
await a.keyboard.type('keep me italic');
await a.setInputFiles('.toolbar input[type=file]', FIXTURES + 'pixel.png');
await a.waitForSelector('.area img[data-upload]');
await a.click('button:has-text("Save draft")');
await a.waitForSelector('text=Draft saved');
await a.click('nav a:has-text("Drafts")');
await a.click(`text=Rich draft ${tag}`);
await a.waitForSelector('.area i, .area em');
await a.waitForSelector('.area img[data-upload]');
step('rich by default; HTML draft reopens with formatting and its image re-hosted');
await a.click('button[aria-label=Discard]');

await browser.close();
finish('folders, reading, rich text', problems);
