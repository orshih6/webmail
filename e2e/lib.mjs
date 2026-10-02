// Shared setup for the browser walkthroughs. Run against the dev stack:
//   dev/up.sh && source dev/dev.env && cargo run   (then)   e2e/run.sh
import { mkdirSync } from 'node:fs';
import { chromium } from 'playwright';

export const BASE = process.env.E2E_BASE ?? 'http://localhost:8080';
export const SHOTS = new URL('./shots/', import.meta.url).pathname;
export const FIXTURES = new URL('./fixtures/', import.meta.url).pathname;
mkdirSync(SHOTS, { recursive: true });

/** Console noise every run produces on purpose: 401s before sign-in / wrong password, and
 *  the fake tracker domain in the seeded newsletter after "Show images". */
export const EXPECTED_NOISE = /401|ERR_NAME_NOT_RESOLVED/;

const openPages = new Set();
let crashHandlerInstalled = false;

/** On an uncaught failure, screenshot every open page (to e2e/shots/, uploaded by CI) and
 *  dump its URL before exiting — a failing step otherwise leaves no evidence of what the
 *  page actually looked like. */
function installCrashHandler() {
  if (crashHandlerInstalled) return;
  crashHandlerInstalled = true;
  process.on('uncaughtException', async (err) => {
    console.error(err);
    const script = process.argv[1].split('/').pop().replace(/\.mjs$/, '');
    let i = 0;
    for (const p of openPages) {
      const name = `${SHOTS}crash-${script}-${i++}.png`;
      await p.screenshot({ path: name, fullPage: true }).catch(() => {});
      console.error(`  page ${i}: ${p.url()} → ${name}`);
    }
    process.exit(1);
  });
}

export async function launch() {
  // System Chrome by default (no download); E2E_CHANNEL=chromium uses Playwright's own.
  const channel = process.env.E2E_CHANNEL ?? 'chrome';
  const browser = await chromium.launch(channel === 'chromium' ? {} : { channel });
  installCrashHandler();
  const newContext = browser.newContext.bind(browser);
  browser.newContext = async (...args) => {
    const ctx = await newContext(...args);
    ctx.on('page', (p) => {
      openPages.add(p);
      p.on('close', () => openPages.delete(p));
    });
    return ctx;
  };
  const newPage = browser.newPage.bind(browser);
  browser.newPage = async (...args) => {
    const p = await newPage(...args);
    openPages.add(p);
    p.on('close', () => openPages.delete(p));
    return p;
  };
  return browser;
}

/** Signs in over the API and returns a tiny client (cookie + CSRF handled). */
export async function apiClient(email, password) {
  const res = await fetch(`${BASE}/api/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email, password })
  });
  if (!res.ok) throw new Error(`login ${email}: ${res.status}`);
  const cookie = res.headers.get('set-cookie').split(';')[0];
  const { csrf } = await res.json();
  return async (method, path, body) => {
    const r = await fetch(`${BASE}${path}`, {
      method,
      headers: { cookie, 'x-csrf-token': csrf, ...(body ? { 'content-type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined
    });
    if (!r.ok) throw new Error(`${method} ${path}: ${r.status} ${await r.text()}`);
    const text = await r.text();
    return text ? JSON.parse(text) : null;
  };
}

/** Puts an account back to a known state: default prefs, no saved identities, empty
 *  address book, no user folders. Mail in system folders is left alone. */
export async function resetAccount(email, password) {
  const api = await apiClient(email, password);
  await api('POST', '/api/prefs', {});
  for (const i of await api('GET', '/api/identities')) if (i.id !== null) await api('DELETE', `/api/identities/${i.id}`);
  for (const c of await api('GET', '/api/contacts')) await api('DELETE', `/api/contacts/${c.id}`);
  const folders = (await api('GET', '/api/folders')).filter((f) => !f.special && f.path !== 'INBOX');
  folders.sort((a, b) => b.path.length - a.path.length); // children first
  for (const f of folders) await api('POST', '/api/folders/delete', { folder: f.path });
  await api('POST', '/api/logout', {});
}

export async function resetAll() {
  await resetAccount('alice@example.test', 'alicepass');
  await resetAccount('bob@example.test', 'bobpass');
}

export function finish(name, problems) {
  if (problems.length) {
    console.error(`✗ ${name}:\n  ${problems.join('\n  ')}`);
    process.exit(1);
  }
  console.log(`✓ ${name}: no unexpected page errors`);
}
