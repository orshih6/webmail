// README screenshots from the real app, on the demo mailbox (dev/seed-demo.py first).
//   node e2e/screenshots.mjs   → docs/screenshots/*.png
import { BASE as base, apiClient, launch } from './lib.mjs';

const out = new URL('../docs/screenshots/', import.meta.url).pathname;
const api = await apiClient('jane@example.test', 'janepass');
await api('POST', '/api/prefs', {});
for (const i of await api('GET', '/api/identities')) if (i.id !== null) await api('DELETE', `/api/identities/${i.id}`);
await api('POST', '/api/identities', { id: null, name: 'Jane Cooper', reply_to: '', signature: 'Jane Cooper\nDesign Lead, Northwind', is_default: true });
for (const c of await api('GET', '/api/contacts')) await api('DELETE', `/api/contacts/${c.id}`);
await api('POST', '/api/contacts', { id: null, name: 'Maria Lopez', email: 'maria@northwind.example' });

const browser = await launch();
async function page(opts) {
  const p = await (await browser.newContext({ deviceScaleFactor: 2, ...opts })).newPage();
  await p.goto(base + '/login');
  await p.fill('input[type=email]', 'jane@example.test');
  await p.fill('input[type=password]', 'janepass');
  await p.click('button[type=submit]');
  await p.waitForURL('**/mail?f=INBOX');
  await p.waitForSelector('.rows li');
  await p.addStyleTag({ content: '.toasts{display:none!important}' });
  return p;
}

// 1. Inbox, light, with a conversation open.
let p = await page({ viewport: { width: 1440, height: 860 }, colorScheme: 'light' });
await p.locator('.rows li .row', { hasText: 'Re: Design review moved to Thursday' }).click();
await p.waitForSelector('section.conversation li >> nth=2');
await p.locator('section.conversation li').nth(1).locator('.item').click();
await p.waitForSelector('section.conversation .body article');
await p.waitForTimeout(400);
await p.screenshot({ path: out + 'inbox-light.png' });

// 2. Rich-text compose, dark.
p = await page({ viewport: { width: 1200, height: 760 }, colorScheme: 'dark' });
await p.click('a:has-text("Compose")');
await p.waitForSelector('textarea');
await p.click('button:has-text("Rich text")');
await p.locator('input[aria-label=To]').pressSequentially('mar', { delay: 40 });
await p.waitForSelector('[role=listbox] li');
await p.keyboard.press('Enter');
await p.fill('label:has-text("Subject") input', 'Prototype notes for Thursday');
await p.locator('.area').focus();
await p.keyboard.type('Hi Maria,');
await p.keyboard.press('Enter');
await p.keyboard.press('Enter');
await p.keyboard.type('Here is what changed since the last review:');
await p.keyboard.press('Enter');
await p.click('button[aria-label="Bulleted list"]');
await p.click('button[aria-label^="Bold"]');
await p.keyboard.type('Search');
await p.click('button[aria-label^="Bold"]');
await p.keyboard.type(' is now instant, even in huge folders');
await p.keyboard.press('Enter');
await p.click('button[aria-label^="Bold"]');
await p.keyboard.type('Undo');
await p.click('button[aria-label^="Bold"]');
await p.keyboard.type(' for delete and move');
await p.keyboard.press('Enter');
await p.click('button[aria-label^="Bold"]');
await p.keyboard.type('Dark mode');
await p.click('button[aria-label^="Bold"]');
await p.keyboard.type(' that follows your system');
await p.keyboard.press('Enter');
await p.keyboard.press('Enter');
await p.keyboard.type('See you Thursday!');
await p.waitForTimeout(300);
await p.screenshot({ path: out + 'compose-dark.png' });

// 3. Phone.
p = await page({ viewport: { width: 390, height: 780 }, colorScheme: 'light', isMobile: true, hasTouch: true });
await p.mouse.move(389, 779); // no stray hover highlight
await p.waitForTimeout(300);
await p.screenshot({ path: out + 'mobile.png' });

await browser.close();
console.log('wrote docs/screenshots/{inbox-light,compose-dark,mobile}.png');
