// Automated accessibility audit (axe-core, WCAG 2.1 A/AA rules) of every main screen, in
// light and dark themes. Fails on any violation of "serious" or "critical" impact and
// prints the rest.
import AxeBuilder from '@axe-core/playwright';
import { BASE as base, apiClient, finish, launch, resetAll } from './lib.mjs';

const problems = [];
const minor = [];
const step = (s) => console.log('✓', s);

async function audit(page, name) {
  const r = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    // Message bodies are third-party HTML in a sandboxed iframe; not ours to fix.
    .exclude('iframe[title="Message body"]')
    .analyze();
  for (const v of r.violations) {
    const where = v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ');
    const line = `[${name}] ${v.impact} ${v.id}: ${v.help} — ${where}${v.nodes.length > 3 ? ` (+${v.nodes.length - 3})` : ''}`;
    (v.impact === 'serious' || v.impact === 'critical' ? problems : minor).push(line);
  }
}

await resetAll();
const browser = await launch();
for (const scheme of ['light', 'dark']) {
  const p = await (await browser.newContext({ viewport: { width: 1400, height: 900 }, colorScheme: scheme })).newPage();
  await p.goto(base + '/login');
  await p.waitForSelector('form');
  await audit(p, `${scheme} login`);
  await p.fill('input[type=email]', 'alice@example.test');
  await p.fill('input[type=password]', 'alicepass');
  await p.click('button[type=submit]');
  await p.waitForURL('**/mail?f=INBOX');
  await p.waitForSelector('.rows li');
  await audit(p, `${scheme} list`);
  await p.locator('.rows li .row').first().click();
  await p.waitForSelector('article h2');
  await audit(p, `${scheme} reader`);
  await p.click('a:has-text("Compose")');
  await p.waitForSelector('textarea, .area');
  await audit(p, `${scheme} compose`);
  for (const tab of ['general', 'identities', 'contacts']) {
    await p.goto(`${base}/mail/settings?tab=${tab}`);
    await p.waitForSelector('.tabs');
    await p.waitForTimeout(300);
    await audit(p, `${scheme} settings/${tab}`);
  }
  await p.goto(base + '/mail?f=INBOX');
  await p.waitForSelector('nav li a');
  const li = p.locator('nav li', { has: p.locator('a', { hasText: 'Inbox' }) });
  await li.hover();
  await li.locator('button[aria-haspopup=menu]').click();
  await audit(p, `${scheme} folder menu`);
  step(`${scheme}: audited login, list, reader, compose, settings ×3, menu`);
}
// ---- Keyboard only ------------------------------------------------------------------------
{
  const p = await (await browser.newContext({ viewport: { width: 1400, height: 900 }, reducedMotion: 'reduce' })).newPage();
  await p.goto(base + '/login');
  await p.fill('input[type=email]', 'alice@example.test');
  await p.fill('input[type=password]', 'alicepass');
  await p.keyboard.press('Enter');
  await p.waitForURL('**/mail?f=INBOX');
  await p.waitForSelector('.rows li');
  const active = () => p.evaluate(() => {
    const a = document.activeElement;
    return { cls: a?.className ?? '', label: a?.getAttribute('aria-label') ?? '', uid: a?.closest('li')?.dataset.uid ?? null, id: a?.id ?? '' };
  });

  await p.evaluate(() => document.activeElement?.blur());
  await p.keyboard.press('Tab');
  if (!(await active()).cls.includes('skip')) problems.push('first Tab stop is not the skip link');
  await p.keyboard.press('Enter');
  let a = await active();
  if (!a.cls.includes('row') || !/^(Unread, )?From /.test(a.label)) problems.push(`skip link landed on ${JSON.stringify(a)}`);
  const first = a.uid;
  step('skip link lands on a message; rows have a full spoken label');

  await p.keyboard.press('ArrowDown');
  a = await active();
  if (a.uid === first || !a.cls.includes('row')) problems.push('ArrowDown did not move focus to the next row');
  if (new URL(p.url()).searchParams.get('m')) problems.push('ArrowDown opened a message');
  await p.keyboard.press('Enter');
  await p.waitForSelector('article h2');
  if (new URL(p.url()).searchParams.get('m') !== a.uid) problems.push('Enter did not open the focused message');
  step('arrows move focus without opening; Enter opens');

  await p.locator(`.rows li[data-uid="${a.uid}"] .row`).focus();
  await p.keyboard.press('#');
  await p.locator(`.rows li[data-uid="${a.uid}"]`).waitFor({ state: 'detached' });
  await p.waitForTimeout(200);
  const after = await active();
  if (!after.cls.includes('row')) problems.push(`after delete focus is on ${JSON.stringify(after)}, not the next message`);
  step('after delete, focus moves to the next message');
  // Put it back for the other suites.
  await p.click('.toast button:has-text("Undo")');

  const said = await p.locator('[aria-live=polite][aria-atomic=true]').textContent();
  if (!/Inbox, \d+ messages/.test(said)) problems.push(`live region said "${said}"`);
  const motion = await p.evaluate(() => getComputedStyle(document.querySelector('nav')).transitionDuration);
  if (!/^0\.00001s|^0s|1e-05s/.test(motion)) problems.push('reduced motion: nav still transitions ' + motion);
  step('folder announced to screen readers; reduced motion disables transitions');

  // Real new mail is announced; our own Undo above was not (checked just before).
  await p.waitForTimeout(5500); // past the "own change" quiet window
  const bob = await apiClient('bob@example.test', 'bobpass');
  await bob('POST', '/api/send', {
    to: 'alice@example.test', cc: '', bcc: '', subject: `Announce me ${Date.now()}`, text: 'x',
    uploads: [], keep: null, in_reply_to: null, references: [], reply_of: null, forward_of: null, draft: null
  });
  await p.waitForFunction(
    () => /new message/.test(document.querySelector('[aria-live=polite][aria-atomic=true]')?.textContent ?? ''),
    null,
    { timeout: 30000 }
  ).catch(() => problems.push('arriving mail was not announced'));
  step('arriving mail is announced; undoing a delete is not');
}

await browser.close();
if (minor.length) console.log('minor/moderate:\n  ' + minor.join('\n  '));
finish('accessibility (axe, WCAG 2.1 AA)', problems);
