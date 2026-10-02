// Conversation view: a thread spread over Inbox and Sent shows as one strip in the reader,
// oldest first, with the other messages expandable in place.
import { BASE as base, EXPECTED_NOISE, apiClient, finish, launch, resetAll } from './lib.mjs';

const problems = [];
const step = (s) => console.log('✓', s);
const tag = Date.now() % 100000;
const topic = `Thread topic ${tag}`;
const blank = { cc: '', bcc: '', uploads: [], keep: null, forward_of: null, draft: null };

await resetAll();
const alice = await apiClient('alice@example.test', 'alicepass');
const bob = await apiClient('bob@example.test', 'bobpass');
async function waitFor(api, folder, subject) {
  for (let i = 0; i < 60; i++) {
    const page = await api('GET', `/api/messages?folder=${encodeURIComponent(folder)}&q=${encodeURIComponent(subject)}`);
    const m = page.messages.find((x) => x.subject === subject);
    if (m) return { ...m, detail: await api('GET', `/api/message?folder=${encodeURIComponent(folder)}&uid=${m.uid}`) };
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`${subject} never arrived in ${folder}`);
}
// bob → alice, alice replies, bob replies again.
await bob('POST', '/api/send', { ...blank, to: 'alice@example.test', subject: topic, text: 'first from bob',
  in_reply_to: null, references: [], reply_of: null });
const first = await waitFor(alice, 'INBOX', topic);
const mid1 = first.detail.message_id;
await alice('POST', '/api/send', { ...blank, to: 'bob@example.test', subject: `Re: ${topic}`, text: 'alice answers',
  in_reply_to: mid1, references: [mid1], reply_of: { folder: 'INBOX', uid: first.uid } });
const atBob = await waitFor(bob, 'INBOX', `Re: ${topic}`);
const mid2 = atBob.detail.message_id;
await bob('POST', '/api/send', { ...blank, to: 'alice@example.test', subject: `Re: ${topic}`, text: 'bob again',
  in_reply_to: mid2, references: [mid1, mid2], reply_of: { folder: 'INBOX', uid: atBob.uid } });
await waitFor(alice, 'INBOX', `Re: ${topic}`);

const browser = await launch();
const p = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
p.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
p.on('console', (m) => m.type() === 'error' && !EXPECTED_NOISE.test(m.text()) && problems.push(m.text()));
await p.goto(base + '/login');
await p.fill('input[type=email]', 'alice@example.test');
await p.fill('input[type=password]', 'alicepass');
await p.click('button[type=submit]');
await p.waitForURL('**/mail?f=INBOX');

// Open bob's latest reply (search keeps the list short and unambiguous).
await p.fill('input[aria-label=Search]', `"bob again"`.replace(/"/g, ''));
await p.keyboard.press('Enter');
await p.locator('.rows li .row', { hasText: `Re: ${topic}` }).first().click();
const strip = p.locator('section.conversation');
await strip.waitFor();
const items = strip.locator('li');
if ((await items.count()) !== 3) problems.push(`expected 3 in the conversation, got ${await items.count()}`);
const whos = await items.locator('.who').allTextContents();
if (!/bob/i.test(whos[0]) || !/alice/i.test(whos[1]) || !/bob/i.test(whos[2])) problems.push('order: ' + whos);
if (!(await items.nth(1).textContent()).includes('Sent')) problems.push("alice's reply not tagged Sent");
if (!(await items.nth(2).textContent()).includes('Open below')) problems.push('open message not marked');
step('conversation: bob → alice (Sent) → bob, oldest first, open one marked');

await items.nth(1).locator('.item').click();
await items.nth(1).locator('.body article').waitFor();
if (!(await items.nth(1).locator('.body').textContent()).includes('alice answers')) problems.push("expanded reply doesn't show its text");
step('expanding an earlier message shows it in place');
await p.screenshot({ path: new URL('./shots/conversation.png', import.meta.url).pathname });

// A message outside any thread gets no strip.
await p.click('nav a:has-text("Inbox")');
await p.locator('.rows li .row', { hasText: 'October product update' }).first().click();
await p.waitForSelector('article h2');
await p.waitForTimeout(800);
if (await p.locator('section.conversation').count()) problems.push('strip shown for a lone message');
step('a lone message shows no conversation strip');

await browser.close();
finish('conversation view', problems);
