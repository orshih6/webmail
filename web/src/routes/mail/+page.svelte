<script lang="ts">
import { SvelteSet } from 'svelte/reactivity';
import { goto } from '$app/navigation';
import { page } from '$app/state';
import { api, qs } from '$lib/api/client';
import type { FlagName } from '$lib/api/types/FlagName';
import type { MessageDetail } from '$lib/api/types/MessageDetail';
import type { MessagePage } from '$lib/api/types/MessagePage';
import type { MessageSummary } from '$lib/api/types/MessageSummary';
import type { MoveResult } from '$lib/api/types/MoveResult';
import type { Prefs } from '$lib/api/types/Prefs';
import type { SearchResults } from '$lib/api/types/SearchResults';
import Conversation from '$lib/components/Conversation.svelte';
import Icon from '$lib/components/Icon.svelte';
import Menu from '$lib/components/Menu.svelte';
import MessageView, { prefetchMessage } from '$lib/components/MessageView.svelte';
import { display, shortDate } from '$lib/format';
import { app, fail, pageTitle, specialPath, toast, undoToast } from '$lib/state.svelte';

// ---- URL is the source of truth: f=folder, m=open uid, q=search, p=page ----------------
const params = $derived(page.url.searchParams);
const folder = $derived(params.get('f') || 'INBOX');
const openUid = $derived(Number(params.get('m')) || null);
const query = $derived(params.get('q') ?? '');
const filter = $derived(params.get('filter') ?? '');
/** scope=all: the search runs across every folder (only meaningful with a query). */
const allMode = $derived(params.get('scope') === 'all' && !!query);
const pageNo = $derived(Math.max(1, Number(params.get('p')) || 1));

function nav(changes: Record<string, string | number | null>, replace = false) {
	const p = new URLSearchParams(page.url.searchParams);
	for (const [k, v] of Object.entries(changes)) {
		if (v === null || v === '') p.delete(k);
		else p.set(k, String(v));
	}
	goto(`/mail?${p}`, { replaceState: replace, keepFocus: true, noScroll: true });
}

// ---- Data -------------------------------------------------------------------------------
const cache = new Map<string, MessagePage>();
let list = $state<MessagePage | null>(null);
let loading = $state(false);
let searchInput = $state('');
const selected = new SvelteSet<number>();

const current = $derived(app.folders.find((f) => f.path === folder));
const showRecipient = $derived(current?.special === 'sent' || current?.special === 'drafts');
const inDrafts = $derived(current?.special === 'drafts');

async function load(key: string, showSpinner: boolean) {
	if (showSpinner) loading = true;
	try {
		const res = await api<MessagePage>(
			'GET',
			`/messages?${qs({ folder, q: query, filter, page: pageNo, page_size: app.prefs.page_size })}`
		);
		cache.set(key, res);
		if (key === listKey()) list = res;
	} catch (e) {
		fail(e);
	} finally {
		loading = false;
	}
}
const listKey = () => `${folder}\n${query}\n${filter}\n${pageNo}\n${app.prefs.page_size}`;

// ---- Search across all folders ------------------------------------------------------------
let hits = $state<SearchResults | null>(null);
const hitsCache = new Map<string, SearchResults>();
const hitsKey = () => `${query}\n${filter}`;

async function loadAll(key: string) {
	loading = true;
	try {
		const res = await api<SearchResults>('GET', `/search?${qs({ q: query, filter })}`);
		hitsCache.set(key, res);
		if (key === hitsKey()) hits = res;
	} catch (e) {
		fail(e);
	} finally {
		loading = false;
	}
}

const folderName = (path: string) => app.folders.find((f) => f.path === path)?.name ?? path;

function setScope(all: boolean) {
	nav({ scope: all ? 'all' : null, m: null, p: null });
}

// Dragging a row onto a folder in the sidebar moves it (and the selection, if it is part of it).
function dragStart(e: DragEvent, m: MessageSummary) {
	const uids = selected.has(m.uid) ? [...selected] : [m.uid];
	app.drag = { folder, uids };
	if (e.dataTransfer) {
		e.dataTransfer.effectAllowed = 'move';
		e.dataTransfer.setData('text/plain', uids.length === 1 ? m.subject : `${uids.length} messages`);
	}
}

const messageUrl = (path: string, extra: Record<string, string | number> = {}) =>
	`/api/message/${path}?${qs({ folder, uid: openUid, ...extra })}`;

// Show the cached page instantly, then revalidate. Live updates bump app.changed.
$effect(() => {
	void app.changed;
	if (allMode) {
		const key = hitsKey();
		hits = hitsCache.get(key) ?? null;
		loadAll(key);
		return;
	}
	const key = listKey();
	const hit = cache.get(key);
	if (hit) list = hit;
	load(key, !hit);
});

// Folder or search change: forget the selection, sync the search box.
$effect(() => {
	void folder;
	searchInput = query;
	selected.clear();
});

// ---- Conversations ---------------------------------------------------------------------
const conversations = $derived(app.prefs.conversations);
const expanded = new SvelteSet<string>();

async function toggleConversations() {
	app.prefs.conversations = !app.prefs.conversations;
	try {
		app.prefs = await api<Prefs>('POST', '/prefs', app.prefs);
	} catch (e) {
		app.prefs.conversations = !app.prefs.conversations;
		fail(e);
	}
}

type Row = { msg: MessageSummary; count: number; child: boolean; unread: boolean };
const rows = $derived.by<Row[]>(() => {
	const msgs = list?.messages ?? [];
	if (!conversations)
		return msgs.map((msg) => ({ msg, count: 1, child: false, unread: !msg.flags.seen }));
	const groups = new Map<string, MessageSummary[]>();
	for (const m of msgs) {
		const g = groups.get(m.thread_key);
		if (g) g.push(m);
		else groups.set(m.thread_key, [m]);
	}
	const out: Row[] = [];
	for (const [key, g] of groups) {
		out.push({ msg: g[0], count: g.length, child: false, unread: g.some((m) => !m.flags.seen) });
		if (g.length > 1 && expanded.has(key))
			for (const m of g.slice(1))
				out.push({ msg: m, count: 1, child: true, unread: !m.flags.seen });
	}
	return out;
});

// ---- Actions (optimistic, rolled back on failure) --------------------------------------
const targets = () => (selected.size ? [...selected] : openUid ? [openUid] : []);

function patch(uids: number[], fn: (m: MessageSummary) => void) {
	for (const m of list?.messages ?? []) if (uids.includes(m.uid)) fn(m);
}

async function setFlag(flag: FlagName, value: boolean, uids = targets()) {
	if (!uids.length) return;
	const before = new Map(
		(list?.messages ?? []).filter((m) => uids.includes(m.uid)).map((m) => [m.uid, { ...m.flags }])
	);
	patch(uids, (m) => (m.flags[flag] = value));
	try {
		await api('POST', '/messages/flag', { folder, uids, flag, value });
		if (flag === 'seen') app.changed++;
	} catch (e) {
		patch(uids, (m) => (m.flags = before.get(m.uid) ?? m.flags));
		fail(e);
	}
}

async function remove(kind: 'delete' | 'junk' | 'move', to?: string) {
	const uids = targets();
	if (!uids.length) return;
	// From Trash (or with no Trash) a delete is permanent: the one action Undo can't reverse.
	const permanent = kind === 'delete' && (current?.special === 'trash' || !specialPath('trash'));
	const n = uids.length === 1 ? 'this message' : `these ${uids.length} messages`;
	if (permanent && !confirm(`Permanently delete ${n}? This cannot be undone.`)) return;
	const snapshot = list?.messages ?? [];
	const nextOpen = allMode ? null : (neighbour(1, uids) ?? neighbour(-1, uids));
	if (list && !allMode) {
		list.messages = list.messages.filter((m) => !uids.includes(m.uid));
		list.total -= uids.length;
	}
	selected.clear();
	if (openUid && uids.includes(openUid)) nav({ m: nextOpen }, true);
	// Keep keyboard users in place: focus moves to the next message, not to <body>.
	refocus(nextOpen);
	try {
		const result =
			kind === 'move'
				? await api<MoveResult>('POST', '/messages/move', { folder, uids, to })
				: await api<MoveResult>('POST', `/messages/${kind}`, { folder, uids });
		cache.clear();
		hitsCache.clear();
		app.changed++;
		const count = uids.length === 1 ? 'Message' : `${uids.length} messages`;
		const where = app.folders.find((f) => f.path === result?.to)?.name ?? result?.to;
		const text = !result?.to
			? `${count} deleted`
			: kind === 'junk'
				? `${count} marked as junk`
				: `${count} moved to ${where}`;
		undoToast(text, folder, result, undoMove);
	} catch (e) {
		if (list && !allMode) {
			list.messages = snapshot;
			list.total += uids.length;
		}
		fail(e);
	}
}

async function undoMove(from: string, uids: number[], back: string) {
	try {
		await api<MoveResult>('POST', '/messages/move', { folder: from, uids, to: back });
		cache.clear();
		app.changed++;
		toast('Undone');
	} catch (e) {
		fail(e);
	}
}

/** The nearest visible message after (dir=1) or before (dir=-1) the open one, skipping `skip`. */
function neighbour(dir: 1 | -1, skip: number[] = []): number | null {
	const ids = rows.map((r) => r.msg.uid);
	let i = openUid ? ids.indexOf(openUid) : -1;
	for (i += dir; i >= 0 && i < ids.length; i += dir) if (!skip.includes(ids[i])) return ids[i];
	return null;
}

function open(m: MessageSummary) {
	if (inDrafts) {
		goto(`/mail/compose?${qs({ mode: 'draft', f: folder, m: m.uid })}`);
		return;
	}
	nav({ m: m.uid });
}

function onloaded(d: MessageDetail) {
	const row = list?.messages.find((m) => m.uid === d.uid);
	if (row && !row.flags.seen) {
		row.flags.seen = true;
		const f = app.folders.find((x) => x.path === folder);
		if (f && f.unread > 0) f.unread--;
	}
}

function respond(mode: 'reply' | 'all' | 'forward') {
	if (openUid) goto(`/mail/compose?${qs({ mode, f: folder, m: openUid })}`);
}

function search(e: SubmitEvent) {
	e.preventDefault();
	nav({ q: searchInput.trim() || null, p: null, m: null });
}

function toggleSelect(uid: number, e: Event) {
	e.stopPropagation();
	if (selected.has(uid)) selected.delete(uid);
	else selected.add(uid);
}

const allSelected = $derived(rows.length > 0 && rows.every((r) => selected.has(r.msg.uid)));
function selectAll() {
	if (allSelected) selected.clear();
	else for (const r of rows) selected.add(r.msg.uid);
}

const moveTargets = $derived(app.folders.filter((f) => f.selectable && f.path !== folder));
function moveTo(e: Event) {
	const el = e.currentTarget as HTMLSelectElement;
	if (el.value) remove('move', el.value);
	el.value = '';
}

const openRow = $derived(list?.messages.find((m) => m.uid === openUid));
const lastPage = $derived(list ? Math.max(1, Math.ceil(list.total / list.page_size)) : 1);

// With a message open, quietly load the next one so j / "next" is instant.
$effect(() => {
	if (!openUid || allMode || inDrafts) return;
	const next = neighbour(1);
	if (next == null) return;
	const f = folder;
	const t = setTimeout(() => prefetchMessage(f, next), 250);
	return () => clearTimeout(t);
});

// ---- Keyboard: the list is one Tab stop; arrows move within it ----------------------------
let focusedUid = $state<number | null>(null);
const tabStop = $derived(
	rows.some((r) => r.msg.uid === focusedUid) ? focusedUid : (openUid ?? rows[0]?.msg.uid ?? null)
);

function focusRow(uid: number | null | undefined) {
	if (uid == null) return;
	document.querySelector<HTMLElement>(`.rows li[data-uid="${uid}"] .row`)?.focus();
}

/** After a row disappears, put focus on its successor if focus was lost with it. */
function refocus(next: number | null) {
	queueMicrotask(() => {
		if (document.activeElement && document.activeElement !== document.body) return;
		focusRow(next ?? rows[0]?.msg.uid);
	});
}

function rowKeys(e: KeyboardEvent) {
	const ids = rows.map((r) => r.msg.uid);
	const i = ids.indexOf(focusedUid ?? -1);
	const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: ids.length - 1 }[e.key];
	if (to === undefined) return;
	e.preventDefault();
	e.stopPropagation(); // arrows here move focus; they don't open messages like j/k
	focusRow(ids[Math.max(0, Math.min(ids.length - 1, to))]);
}

function rowLabel(r: Row) {
	const m = r.msg;
	const who = showRecipient
		? `To ${m.to.map(display).join(', ') || 'nobody'}`
		: `From ${display(m.from) || 'unknown'}`;
	return [
		r.unread ? 'Unread' : null,
		who,
		m.subject || 'No subject',
		shortDate(m.date),
		m.has_attachments ? 'has attachments' : null,
		m.flags.flagged ? 'starred' : null,
		m.flags.answered ? 'replied' : null,
		r.count > 1 ? `${r.count} in conversation` : null
	]
		.filter(Boolean)
		.join(', ');
}

// Screen readers hear which folder they are in and how much is in it — once per folder or
// search, not on every background refresh.
let announcedFor = '';
$effect(() => {
	if (!list || allMode) return;
	const key = `${folder}\n${query}\n${filter}`;
	if (key === announcedFor) return;
	announcedFor = key;
	const unread = current?.unread ? `, ${current.unread} unread` : '';
	app.announcement = query
		? `${list.total} results for ${query}`
		: `${current?.name ?? folder}, ${list.total} messages${unread}`;
});

// ---- Keyboard ---------------------------------------------------------------------------
let searchEl = $state<HTMLInputElement>();
function onkey(e: KeyboardEvent) {
	const t = e.target as HTMLElement;
	if (e.metaKey || e.ctrlKey || e.altKey || t.closest('input, textarea, select, [contenteditable]'))
		return;
	const go = (uid: number | null) => uid && nav({ m: uid }, true);
	const actions: Record<string, () => void> = {
		j: () => go(neighbour(1) ?? (openUid ? null : (rows[0]?.msg.uid ?? null))),
		ArrowDown: () => go(neighbour(1) ?? (openUid ? null : (rows[0]?.msg.uid ?? null))),
		k: () => go(neighbour(-1)),
		ArrowUp: () => go(neighbour(-1)),
		r: () => respond('reply'),
		a: () => respond('all'),
		f: () => respond('forward'),
		c: () => goto('/mail/compose'),
		s: () => openRow && setFlag('flagged', !openRow.flags.flagged),
		u: () => {
			setFlag('seen', false);
			nav({ m: null });
		},
		'#': () => remove('delete'),
		x: () => {
			const uid = focusedUid ?? openUid;
			if (uid) selected.has(uid) ? selected.delete(uid) : selected.add(uid);
		},
		Delete: () => remove('delete'),
		'!': () => remove('junk'),
		'/': () => searchEl?.focus(),
		Escape: () => (selected.size ? selected.clear() : nav({ m: null }))
	};
	const act = actions[e.key];
	if (act) {
		e.preventDefault();
		act();
	}
}
</script>

<svelte:window onkeydown={onkey} />
<svelte:head><title>{pageTitle(`${current?.name ?? 'Mail'}${current?.unread ? ` (${current.unread})` : ''}`)}</title></svelte:head>

<div class="mail" class:reading={openUid !== null}>
	<section class="list" aria-label="Messages" id="messages" tabindex="-1">
		{#if selected.size}
			<div class="bar">
				<input type="checkbox" checked={allSelected} onchange={selectAll} aria-label="Select all" />
				<span class="sel">{selected.size} selected</span>
				<button class="icon-btn" title="Mark read" onclick={() => setFlag('seen', true)}><Icon name="mailOpen" /></button>
				<button class="icon-btn" title="Mark unread" onclick={() => setFlag('seen', false)}><Icon name="mail" /></button>
				<button class="icon-btn" title="Star" onclick={() => setFlag('flagged', true)}><Icon name="star" /></button>
				<select class="move" onchange={moveTo} aria-label="Move to folder">
					<option value="">Move to…</option>
					{#each moveTargets as f (f.path)}<option value={f.path}>{f.name}</option>{/each}
				</select>
				{#if current?.special !== 'junk'}
					<button class="icon-btn" title="Junk (!)" onclick={() => remove('junk')}><Icon name="junk" /></button>
				{/if}
				<button class="icon-btn danger" title="Delete (#)" onclick={() => remove('delete')}><Icon name="trash" /></button>
				<span class="grow"></span>
				<button class="icon-btn" title="Clear selection" onclick={() => selected.clear()}><Icon name="x" /></button>
			</div>
		{:else}
			<div class="bar">
				<button class="icon-btn menu" aria-label="Folders" onclick={() => (app.drawer = true)}><Icon name="menu" /></button>
				<form class="search" role="search" onsubmit={search}>
					<Icon name="search" size={16} />
					<input
						bind:this={searchEl}
						bind:value={searchInput}
						type="search"
						placeholder="Search {current?.name ?? 'mail'}"
						aria-label="Search"
					/>
				</form>
				<button
					class="icon-btn"
					class:on={conversations}
					title={conversations ? 'Conversations on' : 'Conversations off'}
					aria-pressed={conversations}
					onclick={toggleConversations}><Icon name="replyAll" /></button
				>
				<button class="icon-btn" title="Refresh" class:spin={loading} onclick={() => app.changed++}>
					<Icon name="refresh" />
				</button>
			</div>
		{/if}

		<div class="filters" role="group" aria-label="Show">
			{#each [['', 'All'], ['unread', 'Unread'], ['flagged', 'Starred']] as [v, label] (v)}
				<button class:on={filter === v} aria-pressed={filter === v} onclick={() => nav({ filter: v || null, p: null, m: null })}
					>{label}</button
				>
			{/each}
		</div>

		{#if query}
			<div class="searching">
				<span>
					Results for “{query}”
					{#if allMode && hits}in all folders ({hits.hits.length}{hits.truncated ? '+' : ''}){/if}
				</span>
				<label class="scope">
					<input type="checkbox" checked={allMode} onchange={(e) => setScope(e.currentTarget.checked)} />
					All folders
				</label>
				<button class="link" onclick={() => nav({ q: null, p: null, scope: null, m: null })}>Clear</button>
			</div>
		{/if}

		{#if allMode}
			<ul class="rows hits" aria-busy={loading}>
				{#each hits?.hits ?? [] as h (`${h.folder}/${h.message.uid}`)}
					{@const m = h.message}
					<li class:unread={!m.flags.seen} class:active={h.folder === folder && m.uid === openUid}>
						<button class="row" onclick={() => nav({ f: h.folder, m: m.uid })}>
							<span class="who">{display(m.from) || '—'}</span>
							<span class="subject">{m.subject || '(no subject)'}</span>
							<span class="date">{shortDate(m.date)}</span>
						</button>
						<span class="icons">
							<span class="in-folder">{folderName(h.folder)}</span>
							{#if m.has_attachments}<Icon name="clip" size={14} />{/if}
							{#if m.flags.flagged}<span class="star on"><Icon name="star" size={15} filled /></span>{/if}
						</span>
					</li>
				{:else}
					{#if !loading}<li class="empty">Nothing matches your search in any folder.</li>{/if}
				{/each}
				{#if hits?.truncated}
					<li class="empty">Showing the newest {hits.hits.length}. Refine the search to see older ones.</li>
				{/if}
			</ul>
		{:else}

		<ul class="rows" aria-busy={loading} aria-label="{current?.name ?? 'Messages'}">
			{#each rows as r (r.msg.uid)}
				{@const m = r.msg}
				<li
					data-uid={m.uid}
					draggable="true"
					ondragstart={(e) => dragStart(e, m)}
					ondragend={() => (app.drag = null)}
					class:unread={r.unread}
					class:active={m.uid === openUid}
					class:checked={selected.has(m.uid)}
					class:child={r.child}
				>
					<button
						class="row"
						onclick={() => open(m)}
						onfocus={() => (focusedUid = m.uid)}
						onkeydown={rowKeys}
						tabindex={m.uid === tabStop ? 0 : -1}
						aria-current={m.uid === openUid ? 'true' : undefined}
						aria-label={rowLabel(r)}
					>
						<span class="who">
							{#if showRecipient}To: {m.to.map(display).join(', ') || '—'}{:else}{display(m.from) || '—'}{/if}
						</span>
						<span class="subject">{m.subject || '(no subject)'}</span>
						<span class="date">{shortDate(m.date)}</span>
					</button>
					<input
						class="check"
						type="checkbox"
						checked={selected.has(m.uid)}
						onclick={(e) => toggleSelect(m.uid, e)}
						aria-label="Select message"
						tabindex="-1"
					/>
					<span class="icons">
						{#if r.count > 1}
							<button
								class="count"
								tabindex="-1"
								title={expanded.has(m.thread_key) ? 'Collapse' : 'Show conversation'}
								onclick={() =>
									expanded.has(m.thread_key) ? expanded.delete(m.thread_key) : expanded.add(m.thread_key)}
								>{r.count}</button
							>
						{/if}
						{#if m.has_attachments}<Icon name="clip" size={14} />{/if}
						{#if m.flags.answered}<Icon name="reply" size={14} />{/if}
						<button
							class="star"
							class:on={m.flags.flagged}
							tabindex="-1"
							aria-label={m.flags.flagged ? 'Unstar' : 'Star'}
							onclick={() => setFlag('flagged', !m.flags.flagged, [m.uid])}
						>
							<Icon name="star" size={15} filled={m.flags.flagged} />
						</button>
					</span>
				</li>
			{:else}
				{#if !loading}
					<li class="empty">{query
							? 'Nothing matches your search.'
							: filter === 'unread'
								? 'No unread messages.'
								: filter === 'flagged'
									? 'No starred messages.'
									: 'No messages here.'}</li>
				{/if}
			{/each}
		</ul>
		{/if}

		{#if !allMode && list && list.total > list.page_size}
			<div class="pager">
				<span>{(pageNo - 1) * list.page_size + 1}–{Math.min(pageNo * list.page_size, list.total)} of {list.total}</span>
				<button class="icon-btn" aria-label="Newer" disabled={pageNo <= 1} onclick={() => nav({ p: pageNo - 1, m: null })}>
					<Icon name="back" />
				</button>
				<button class="icon-btn" aria-label="Older" disabled={pageNo >= lastPage} onclick={() => nav({ p: pageNo + 1, m: null })}>
					<Icon name="chevron" />
				</button>
			</div>
		{/if}
	</section>

	<section class="reader" aria-label="Message">
		{#if openUid}
			<div class="bar">
				<button class="icon-btn back" aria-label="Back to list" onclick={() => nav({ m: null })}><Icon name="back" /></button>
				<button class="btn reply" onclick={() => respond('reply')} aria-label="Reply"
					><Icon name="reply" size={16} /> <span>Reply</span></button
				>
				<button class="icon-btn" title="Reply all (a)" onclick={() => respond('all')}><Icon name="replyAll" /></button>
				<button class="icon-btn" title="Forward (f)" onclick={() => respond('forward')}><Icon name="forward" /></button>
				<span class="grow"></span>
				{#if openRow}
					<button
						class="icon-btn star"
						class:on={openRow.flags.flagged}
						title="Star (s)"
						onclick={() => setFlag('flagged', !openRow.flags.flagged, [openRow.uid])}
						><Icon name="star" filled={openRow.flags.flagged} /></button
					>
				{/if}
				<button
					class="icon-btn"
					title="Mark unread (u)"
					onclick={() => {
						setFlag('seen', false, [openUid]);
						nav({ m: null });
					}}><Icon name="mail" /></button
				>
				<select class="move" onchange={moveTo} aria-label="Move to folder">
					<option value="">Move…</option>
					{#each moveTargets as f (f.path)}<option value={f.path}>{f.name}</option>{/each}
				</select>
				{#if current?.special !== 'junk' && specialPath('junk')}
					<button class="icon-btn" title="Junk (!)" onclick={() => remove('junk')}><Icon name="junk" /></button>
				{/if}
				<button class="icon-btn danger" title="Delete (#)" onclick={() => remove('delete')}><Icon name="trash" /></button>
				<Menu
					label="More actions"
					items={[
						{ label: 'Print', run: () => window.open(messageUrl('print'), '_blank', 'noopener') },
						{ label: 'View source', run: () => window.open(messageUrl('raw'), '_blank', 'noopener') },
						{ label: 'Download (.eml)', run: () => (location.href = messageUrl('raw', { download: 1 })) }
					]}
				>
					{#snippet trigger()}<Icon name="more" />{/snippet}
				</Menu>
			</div>
			{#key `${folder}/${openUid}`}
				<Conversation {folder} uid={openUid} />
				<MessageView {folder} uid={openUid} {onloaded} />
			{/key}
		{:else}
			<div class="nothing">
				<Icon name="mail" size={40} />
				<p>Select a message to read it</p>
				<p class="keys">j / k to move · r reply · c compose · / search</p>
			</div>
		{/if}
	</section>
</div>

<style>
	.mail {
		flex: 1;
		display: flex;
		min-width: 0;
	}
	.list {
		width: 420px;
		flex: none;
		display: flex;
		flex-direction: column;
		border-right: 1px solid var(--border);
		min-height: 0;
	}
	.reader {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 4px;
		height: 52px;
		flex: none;
		padding: 0 10px;
		border-bottom: 1px solid var(--border);
	}
	.grow {
		flex: 1;
	}
	.sel {
		margin: 0 8px 0 4px;
		font-weight: 600;
		white-space: nowrap;
	}
	.search {
		flex: 1;
		display: flex;
		align-items: center;
		gap: 8px;
		height: 34px;
		padding: 0 10px;
		border-radius: var(--radius);
		background: var(--bg-soft);
		color: var(--text-faint);
	}
	.search:focus-within {
		box-shadow: 0 0 0 2px var(--accent);
	}
	.search input {
		flex: 1;
		min-width: 0;
		border: 0;
		outline: none;
		background: none;
		color: var(--text);
	}
	.move {
		height: 32px;
		max-width: 110px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg);
		padding: 0 6px;
	}
	.icon-btn.on {
		color: var(--accent);
	}
	.icon-btn.danger:hover {
		color: var(--danger);
	}
	.spin :global(svg) {
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	.menu,
	.back {
		display: none;
	}
	.filters {
		display: flex;
		gap: 4px;
		padding: 6px 10px;
		border-bottom: 1px solid var(--border);
	}
	.filters button {
		height: 26px;
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: 13px;
		background: none;
		color: var(--text-soft);
		font-size: 12.5px;
		font-weight: 500;
	}
	.filters button:hover {
		background: var(--bg-hover);
	}
	.filters button.on {
		border-color: var(--accent);
		background: var(--accent-soft);
		color: var(--accent);
	}
	.searching {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.searching span {
		flex: 1;
	}
	.scope {
		display: flex;
		align-items: center;
		gap: 4px;
		cursor: pointer;
		white-space: nowrap;
	}
	.hits .row {
		padding-left: 16px;
	}
	.in-folder {
		padding: 1px 7px;
		border-radius: 9px;
		background: var(--bg-active);
		color: var(--text-soft);
		font-size: 11px;
		font-weight: 600;
		max-width: 120px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.searching {
		padding: 8px 16px;
		font-size: 13px;
		color: var(--text-soft);
		background: var(--bg-soft);
		border-bottom: 1px solid var(--border);
	}
	.link {
		border: 0;
		background: none;
		color: var(--accent);
		font-weight: 600;
		padding: 0 4px;
	}
	.rows {
		list-style: none;
		margin: 0;
		padding: 0;
		overflow-y: auto;
		flex: 1;
	}
	.rows li {
		position: relative;
		border-bottom: 1px solid var(--border);
	}
	.rows li:hover {
		background: var(--bg-hover);
	}
	.rows li.active {
		background: var(--bg-active);
	}
	.rows li.checked {
		background: var(--accent-soft);
	}
	.row {
		display: grid;
		grid-template-columns: 1fr auto;
		grid-template-areas: 'who date' 'subject subject';
		gap: 2px 8px;
		width: 100%;
		padding: 10px 16px 10px 40px;
		border: 0;
		background: none;
		text-align: left;
		color: var(--text-soft);
	}
	.child .row {
		padding-left: 56px;
	}
	.who,
	.subject {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.who {
		grid-area: who;
		color: var(--text);
		padding-right: 70px;
	}
	.subject {
		grid-area: subject;
		padding-right: 56px;
	}
	.date {
		grid-area: date;
		font-size: 12px;
		color: var(--text-faint);
	}
	.unread .who,
	.unread .subject {
		font-weight: 650;
		color: var(--text);
	}
	.unread::before {
		content: '';
		position: absolute;
		left: 0;
		top: 0;
		bottom: 0;
		width: 3px;
		background: var(--accent);
	}
	.check {
		position: absolute;
		left: 14px;
		top: 13px;
		margin: 0;
	}
	.icons {
		position: absolute;
		right: 12px;
		bottom: 8px;
		display: flex;
		align-items: center;
		gap: 6px;
		color: var(--text-faint);
	}
	.count {
		min-width: 20px;
		height: 18px;
		padding: 0 5px;
		border: 1px solid var(--border);
		border-radius: 9px;
		background: var(--bg);
		font-size: 11px;
		font-weight: 700;
		color: var(--text-soft);
	}
	.star {
		display: grid;
		place-items: center;
		border: 0;
		background: none;
		padding: 2px;
		color: var(--text-faint);
	}
	.star.on {
		color: var(--star);
	}
	.empty {
		padding: 48px 16px;
		text-align: center;
		color: var(--text-faint);
		border: 0 !important;
		background: none !important;
	}
	.pager {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 4px;
		padding: 6px 10px;
		border-top: 1px solid var(--border);
		font-size: 12.5px;
		color: var(--text-soft);
	}
	.pager span {
		margin-right: 6px;
	}
	.nothing {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		color: var(--text-faint);
	}
	.nothing p {
		margin: 8px 0 0;
	}
	.keys {
		font-size: 12px;
	}
	@media (max-width: 1100px) {
		.list {
			width: 340px;
		}
	}
	@media (max-width: 760px) {
		.list {
			width: 100%;
			border-right: 0;
		}
		.reader {
			display: none;
		}
		.reading .list {
			display: none;
		}
		.reading .reader {
			display: flex;
		}
		.menu,
		.back {
			display: inline-grid;
		}
		.reader .bar {
			gap: 0;
			padding: 0 4px;
			overflow-x: auto;
			scrollbar-width: none;
		}
		.reply {
			padding: 0 8px;
		}
		.reply span {
			display: none;
		}
		.move {
			max-width: 76px;
		}
	}
</style>
