import { BASE as base, EXPECTED_NOISE, FIXTURES, SHOTS, finish, launch, resetAll } from './lib.mjs';
const tag = Date.now() % 100000;
const problems = [];
const step = (s) => console.log('✓', s);
await resetAll();
const browser = await launch();
async function login(email, pass, opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 860 }, ...opts });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => problems.push(`${email} pageerror: ${e.message}`));
  p.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(`${email}: ${m.text()}`));
  await p.goto(base + '/login');
  await p.fill('input[type=email]', email);
  await p.fill('input[type=password]', pass);
  await p.click('button[type=submit]');
  await p.waitForURL('**/mail?f=INBOX');
  await p.waitForSelector('nav li a');
  return p;
}
const a = await login('alice@example.test', 'alicepass');
// Reset state a previous (aborted) run may have left: prefs, saved contacts, identities.
await a.evaluate(async () => {
  const { csrf } = await (await fetch('/api/session')).json();
  const h = { 'content-type': 'application/json', 'x-csrf-token': csrf };
  await fetch('/api/prefs', { method: 'POST', headers: h, body: '{}' });
  for (const c of await (await fetch('/api/contacts')).json()) await fetch(`/api/contacts/${c.id}`, { method: 'DELETE', headers: h });
  for (const i of await (await fetch('/api/identities')).json()) if (i.id !== null) await fetch(`/api/identities/${i.id}`, { method: 'DELETE', headers: h });
  await fetch('/api/contacts', { method: 'POST', headers: h, body: JSON.stringify({ id: null, name: 'Bob', email: 'bob@example.test' }) });
});
const api = (method, path, body) => a.evaluate(async ([method, path, body]) => {
  const { csrf } = await (await fetch('/api/session')).json();
  const r = await fetch(path, { method, headers: { 'content-type': 'application/json', 'x-csrf-token': csrf }, body: body ? JSON.stringify(body) : undefined });
  return r.status === 204 ? null : r.json();
}, [method, path, body]);
await a.reload();
await a.waitForSelector('nav li a');

// Identities
await a.click('nav a:has-text("Settings")');
await a.click('.tabs a:has-text("Identities")');
await a.click('button:has-text("Edit")');
await a.fill('label:has-text("Your name") input', `Alice ${tag}`);
await a.fill('label:has-text("Signature") textarea', 'Cheers,\nAlice');
await a.click('button:has-text("Save")');
await a.waitForSelector(`.cards >> text=Alice ${tag}`);
await a.click('button:has-text("Add identity")');
await a.fill('label:has-text("Your name") input', `Support ${tag}`);
await a.fill('label:has-text("Reply-To") input', 'support@example.test');
await a.fill('label:has-text("Signature") textarea', 'The Support Team');
await a.click('button:has-text("Save")');
await a.waitForSelector(`.cards >> text=Support ${tag}`);
await a.screenshot({ path: SHOTS + 'p2-identities.png' });
step('two identities created');

// Compose: signature, identity switch, autocomplete
await a.click('a:has-text("Compose")');
await a.waitForFunction(() => document.querySelector('textarea')?.value.includes('-- \nCheers,\nAlice'));
step('default signature inserted');
await a.selectOption('select.from', { label: `Support ${tag} <alice@example.test>` });
await a.waitForFunction(() => { const v = document.querySelector('textarea').value; return v.includes('The Support Team') && !v.includes('Cheers'); });
step('switching identity swaps the signature');
await a.locator('input[aria-label=To]').pressSequentially('bob@ex', { delay: 30 });
await a.waitForSelector('[role=listbox] li:has-text("bob@example.test")');
await a.screenshot({ path: SHOTS + 'p2-autocomplete.png' });
await a.keyboard.press('Enter');
const toVal = await a.inputValue('input[aria-label=To]');
if (!/bob@example\.test>?, $/.test(toVal)) problems.push('autocomplete value: ' + JSON.stringify(toVal));
step('autocomplete picks a contact: ' + JSON.stringify(toVal));
await a.fill('label:has-text("Subject") input', `From support ${tag}`);
await a.keyboard.press('Control+Enter');
await a.waitForSelector('text=Message sent');

const b = await login('bob@example.test', 'bobpass');
await b.waitForSelector(`text=From support ${tag}`, { timeout: 30000 });
const fromName = await b.locator('.rows li', { hasText: `From support ${tag}` }).locator('.who').textContent();
if (!fromName.includes(`Support ${tag}`)) problems.push('bob sees from: ' + fromName);
await b.click(`text=From support ${tag}`);
await b.waitForSelector('.text');
const bodyText = await b.textContent('.text');
if (!bodyText.includes('The Support Team')) problems.push('signature missing in received mail');
step('bob receives it from the chosen identity, with its signature');

// Remote images for contacts: start with bob not in the address book.
for (const c of await api('GET', '/api/contacts')) await api('DELETE', `/api/contacts/${c.id}`);
await a.click('nav a:has-text("Settings")');
await a.click('label:has-text("Show them for people in my contacts")');
await a.waitForTimeout(400);
await a.click('nav a:has-text("Inbox")');
await a.click('text=October product update');
await a.waitForSelector('text=Remote images are hidden');
await a.click('button[aria-label="Add sender to contacts"]');
await a.waitForSelector('text=added to contacts');
await a.click('nav a:has-text("Inbox")');
await a.click('text=October product update');
await a.frameLocator('iframe').locator('h1').waitFor();
await a.waitForTimeout(800);
if (await a.locator('text=Remote images are hidden').count()) problems.push('images still blocked for a contact');
step('images load automatically for a saved contact');

// Contacts tab
await a.click('nav a:has-text("Contacts")');
await a.waitForSelector('.contacts li:has-text("bob@example.test")');
await a.screenshot({ path: SHOTS + 'p2-contacts.png' });
step('contacts list');

// Theme + page size
await a.click('nav a:has-text("Settings")');
await a.click('label:has-text("Dark")');
await a.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
await a.selectOption('label:has-text("Messages per page") select', '25');
await a.waitForTimeout(400);
await a.screenshot({ path: SHOTS + 'p2-settings-dark.png' });
const fresh = await a.context().newPage();
await fresh.goto(base + '/mail?f=INBOX');
await fresh.waitForSelector('nav li a');
const req = fresh.waitForRequest((r) => r.url().includes('/api/messages') && r.url().includes('page_size=25'));
await fresh.click('nav a:has-text("Sent")');
await req;
if ((await fresh.evaluate(() => document.documentElement.dataset.theme)) !== 'dark') problems.push('theme not applied on reload');
step('dark theme and page size persist across reloads');

// Clean up
await a.click('label:has-text("Match system")');
await a.click('label:has-text("Always ask before showing")');
await a.selectOption('label:has-text("Messages per page") select', '50');
await a.click('.tabs a:has-text("Identities")');
await a.waitForSelector(`.cards >> text=Support ${tag}`);
for (const i of await api('GET', '/api/identities')) if (i.id !== null) await api('DELETE', `/api/identities/${i.id}`);
await browser.close();
finish('settings, identities, contacts', problems);
