// Times the API on a large mailbox (dev/seed-large.py first). Prints median / p95 in ms.
//   node e2e/bench.mjs [runs]
import { apiClient } from './lib.mjs';

const runs = Number(process.argv[2] ?? 15);
const api = await apiClient('carol@example.test', 'carolpass');
const q = (p) => `/api/messages?${new URLSearchParams({ folder: 'INBOX', page_size: 50, ...p })}`;
const first = await api('GET', q({}));
const uid = first.messages[25].uid;
console.log(`INBOX: ${first.total} messages`);

const cases = [
  ['folders (all counts)', () => api('GET', '/api/folders')],
  ['list page 1', () => api('GET', q({}))],
  ['list page 2', () => api('GET', q({ page: 2 }))],
  ['list page 200', () => api('GET', q({ page: 200 }))],
  ['unread filter', () => api('GET', q({ filter: 'unread' }))],
  ['search, repeated (cached)', () => api('GET', q({ q: 'invoice' }))],
  // A different term every time: what a user waits for on a new search.
  ['search, new term (uncached)', () => api('GET', q({ q: `#${Math.floor(Math.random() * 20000)}` }))],
  ['open a message again (cached)', () => api('GET', `/api/message?folder=INBOX&uid=${uid}`)],
  // A different message every time: the first open of anything.
  ['open a new message (uncached)', () => api('GET', `/api/message?folder=INBOX&uid=${first.messages[0].uid - 1 - Math.floor(Math.random() * 15000)}`)]
];
const rows = [];
for (const [name, fn] of cases) {
  await fn(); // warm
  const t = [];
  for (let i = 0; i < runs; i++) {
    const s = performance.now();
    await fn();
    t.push(performance.now() - s);
  }
  t.sort((a, b) => a - b);
  rows.push({ case: name, median: Math.round(t[t.length >> 1]), p95: Math.round(t[Math.floor(t.length * 0.95)]) });
}
console.table(rows);
