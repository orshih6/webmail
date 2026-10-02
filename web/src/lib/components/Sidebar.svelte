<script lang="ts">
import { goto } from '$app/navigation';
import { page } from '$app/state';
import { api } from '$lib/api/client';
import type { Folder } from '$lib/api/types/Folder';
import type { MoveResult } from '$lib/api/types/MoveResult';
import { folderUrl } from '$lib/format';
import { app, brand, clearComposeBackups, fail, toast, undoToast } from '$lib/state.svelte';
import Icon, { type IconName } from './Icon.svelte';
import Menu, { type MenuItem } from './Menu.svelte';
import { forgetMessages } from './MessageView.svelte';
import Skeleton from './Skeleton.svelte';

let { onrefresh }: { onrefresh: () => void } = $props();

const icons: Record<string, IconName> = {
	inbox: 'inbox',
	sent: 'send',
	drafts: 'draft',
	trash: 'trash',
	junk: 'junk',
	archive: 'archive'
};

const current = $derived(page.url.pathname === '/mail' ? page.url.searchParams.get('f') : null);

const depth = (f: Folder) => (f.delimiter ? f.path.split(f.delimiter).length - 1 : 0);
const hasChildren = (f: Folder) =>
	!!f.delimiter && app.folders.some((x) => x.path.startsWith(f.path + f.delimiter));

// ---- Inline create / rename -------------------------------------------------------------
type Edit = { mode: 'create'; parent: string | null } | { mode: 'rename'; folder: Folder };
let edit = $state<Edit | null>(null);
let editValue = $state('');
let busy = $state(false);

function startCreate(parent: string | null) {
	edit = { mode: 'create', parent };
	editValue = '';
}
function startRename(f: Folder) {
	edit = { mode: 'rename', folder: f };
	editValue = f.name;
}

async function commit() {
	if (!edit || busy) return;
	const name = editValue.trim();
	if (!name || (edit.mode === 'rename' && name === edit.folder.name)) {
		edit = null;
		return;
	}
	busy = true;
	try {
		if (edit.mode === 'create') {
			await api('POST', '/folders/create', { parent: edit.parent, name });
			toast(`Folder “${name}” created`);
		} else {
			const was = edit.folder.path;
			await api('POST', '/folders/rename', { folder: was, name });
			// If we were inside the renamed folder (or a child), follow it.
			if (current && (current === was || current.startsWith(was + edit.folder.delimiter))) {
				goto(folderUrl('INBOX'));
			}
		}
		edit = null;
		onrefresh();
	} catch (e) {
		fail(e);
	} finally {
		busy = false;
	}
}

function editKey(e: KeyboardEvent) {
	if (e.key === 'Enter') commit();
	else if (e.key === 'Escape') edit = null;
	else return;
	e.preventDefault();
	e.stopPropagation();
}

/** Focus at once (no lost keystrokes); select once the bound value has landed, so a rename
 *  starts with the old name selected. */
function focusAndSelect(el: HTMLInputElement) {
	el.focus();
	queueMicrotask(() => {
		if (edit?.mode === 'rename' && el.value === editValue) el.select();
	});
}

// ---- Folder actions ---------------------------------------------------------------------
async function run(path: string, body: object, done: string) {
	try {
		await api('POST', path, body);
		toast(done);
		onrefresh();
	} catch (e) {
		fail(e);
	}
}

function remove(f: Folder) {
	const what = f.total ? ` and the ${f.total} message${f.total === 1 ? '' : 's'} in it` : '';
	if (!confirm(`Delete the folder “${f.name}”${what}? This cannot be undone.`)) return;
	if (current === f.path) goto(folderUrl('INBOX'));
	run('/folders/delete', { folder: f.path }, `Folder “${f.name}” deleted`);
}

function items(f: Folder): MenuItem[] {
	const out: MenuItem[] = [];
	if (f.unread > 0)
		out.push({
			label: 'Mark all as read',
			run: () => run('/folders/read', { folder: f.path }, `${f.name}: all read`)
		});
	if (f.delimiter) out.push({ label: 'New subfolder…', run: () => startCreate(f.path) });
	if (!f.special) {
		out.push({ label: 'Rename…', run: () => startRename(f) });
		if (!hasChildren(f))
			out.push('separator', { label: 'Delete folder', danger: true, run: () => remove(f) });
	}
	if ((f.special === 'trash' || f.special === 'junk') && f.total > 0) {
		out.push('separator', {
			label: `Empty ${f.name}`,
			danger: true,
			run: () => {
				if (confirm(`Permanently delete all ${f.total} messages in ${f.name}?`))
					run('/folders/empty', { folder: f.path }, `${f.name} emptied`);
			}
		});
	}
	return out;
}

// ---- Drag & drop from the message list --------------------------------------------------
let dropTarget = $state<string | null>(null);

function canDrop(f: Folder) {
	return !!app.drag && f.selectable && f.path !== app.drag.folder;
}

async function drop(e: DragEvent, f: Folder) {
	e.preventDefault();
	dropTarget = null;
	if (!canDrop(f)) return;
	const d = app.drag;
	app.drag = null;
	if (!d) return;
	try {
		const result = await api<MoveResult>('POST', '/messages/move', {
			folder: d.folder,
			uids: d.uids,
			to: f.path
		});
		onrefresh();
		undoToast(
			`Moved ${d.uids.length === 1 ? 'message' : `${d.uids.length} messages`} to ${f.name}`,
			d.folder,
			result,
			async (from, uids, back) => {
				try {
					await api('POST', '/messages/move', { folder: from, uids, to: back });
					onrefresh();
					toast('Undone');
				} catch (e) {
					fail(e);
				}
			}
		);
	} catch (err) {
		fail(err);
	}
}

async function logout() {
	clearComposeBackups();
	forgetMessages();
	try {
		await api('POST', '/logout', {});
	} catch {}
	app.email = '';
	app.folders = [];
	goto('/login');
}
</script>

{#snippet editor(level: number)}
	<li class="editing" style:--depth={level}>
		<Icon name="folder" size={17} />
		<input
			use:focusAndSelect
			bind:value={editValue}
			onkeydown={editKey}
			onblur={commit}
			disabled={busy}
			aria-label={edit?.mode === 'rename' ? 'New folder name' : 'Folder name'}
			placeholder="Folder name"
		/>
	</li>
{/snippet}

<nav class:open={app.drawer} aria-label="Folders">
	<a class="brand" href="/mail?f=INBOX" onclick={() => (app.drawer = false)}>
		<img src="/icons/logo.svg" alt="" width="26" height="26" />
		<span>{brand.app_name}</span>
	</a>
	<a class="btn primary compose" href="/mail/compose" onclick={() => (app.drawer = false)}>
		<Icon name="pen" size={16} /> Compose
	</a>

	<div class="heading">
		<span>Folders</span>
		<button class="icon-btn small" title="New folder" aria-label="New folder" onclick={() => startCreate(null)}>
			<Icon name="plus" size={15} />
		</button>
	</div>

	<ul aria-busy={!app.folders.length}>
		{#if !app.folders.length}<li><Skeleton kind="folders" count={5} /></li>{/if}
		{#each app.folders as f (f.path)}
			{#if edit?.mode === 'rename' && edit.folder.path === f.path}
				{@render editor(depth(f))}
			{:else}
				<li style:--depth={depth(f)} class:drop={dropTarget === f.path}>
					{#if f.selectable}
						<a
							href={folderUrl(f.path)}
							class:active={current === f.path}
							class:unread={f.unread > 0}
							onclick={() => (app.drawer = false)}
							aria-current={current === f.path ? 'page' : undefined}
							ondragover={(e) => {
								if (!canDrop(f)) return;
								e.preventDefault();
								if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
								dropTarget = f.path;
							}}
							ondragleave={() => dropTarget === f.path && (dropTarget = null)}
							ondrop={(e) => drop(e, f)}
						>
							<Icon name={f.special ? icons[f.special] : 'folder'} size={17} />
							<span class="name">{f.name}</span>
							{#if f.unread > 0 && f.special !== 'sent' && f.special !== 'trash'}
								<span class="count">{f.unread}</span>
							{:else if f.special === 'drafts' && f.total > 0}
								<span class="count muted">{f.total}</span>
							{/if}
						</a>
					{:else}
						<span class="group"><Icon name="folder" size={17} /> <span class="name">{f.name}</span></span>
					{/if}
					{#if items(f).length}
						<span class="actions">
							<Menu label="Folder actions for {f.name}" items={items(f)}>
								{#snippet trigger()}<Icon name="more" size={16} />{/snippet}
							</Menu>
						</span>
					{/if}
				</li>
			{/if}
			{#if edit?.mode === 'create' && edit.parent === f.path}
				{@render editor(depth(f) + 1)}
			{/if}
		{/each}
		{#if edit?.mode === 'create' && edit.parent === null}
			{@render editor(0)}
		{/if}
	</ul>

	<div class="links">
		<a href="/mail/settings?tab=contacts" onclick={() => (app.drawer = false)}><Icon name="users" size={17} /> Contacts</a>
		<a href="/mail/settings" onclick={() => (app.drawer = false)}><Icon name="sliders" size={17} /> Settings</a>
	</div>

	<div class="account">
		<span class="who" title={app.email}>{app.email}</span>
		<button class="icon-btn" title="Sign out" aria-label="Sign out" onclick={logout}>
			<Icon name="logout" size={17} />
		</button>
	</div>
</nav>
{#if app.drawer}
	<button class="scrim" aria-label="Close folders" onclick={() => (app.drawer = false)}></button>
{/if}

<style>
	nav {
		width: 240px;
		flex: none;
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 14px 10px 10px;
		background: var(--bg-soft);
		border-right: 1px solid var(--border);
		overflow: hidden;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 2px 8px 6px;
		color: var(--text);
		text-decoration: none;
		font-size: 16px;
		font-weight: 700;
		letter-spacing: -0.01em;
	}
	.brand span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.compose {
		height: 40px;
		justify-content: center;
		text-decoration: none;
		margin: 0 4px 6px;
	}
	.heading {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0 4px 0 10px;
		font-size: 11.5px;
		font-weight: 600;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		color: var(--text-faint);
	}
	.icon-btn.small {
		width: 26px;
		height: 26px;
	}
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		overflow-y: auto;
		flex: 1;
	}
	li {
		position: relative;
		padding-left: calc(var(--depth) * 14px);
	}
	li a,
	.group,
	.editing {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 34px;
		padding: 0 34px 0 10px;
		border-radius: var(--radius);
		color: var(--text-soft);
		text-decoration: none;
	}
	.editing {
		padding: 0 6px 0 calc(10px + var(--depth) * 14px);
	}
	.editing input {
		flex: 1;
		min-width: 0;
		height: 28px;
		padding: 0 8px;
		border: 1px solid var(--accent);
		border-radius: 6px;
		background: var(--bg);
		outline: none;
	}
	li a:hover {
		background: var(--bg-hover);
		color: var(--text);
	}
	li a.active {
		background: var(--bg-active);
		color: var(--text);
		font-weight: 600;
	}
	li.drop a {
		background: var(--accent-soft);
		box-shadow: inset 0 0 0 2px var(--accent);
		color: var(--text);
	}
	li a.unread .name {
		color: var(--text);
		font-weight: 600;
	}
	.name {
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.count {
		font-size: 12px;
		font-weight: 700;
		color: var(--accent);
	}
	.count.muted {
		color: var(--text-faint);
		font-weight: 500;
	}
	.actions {
		position: absolute;
		right: 2px;
		top: 1px;
		opacity: 0;
	}
	li:hover .actions,
	.actions:focus-within {
		opacity: 1;
	}
	li:hover .count {
		visibility: hidden;
	}
	.links {
		display: flex;
		flex-direction: column;
	}
	.links a {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius);
		color: var(--text-soft);
		text-decoration: none;
	}
	.links a:hover {
		background: var(--bg-hover);
		color: var(--text);
	}
	.account {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 8px 4px 0 10px;
		border-top: 1px solid var(--border);
		font-size: 13px;
		color: var(--text-soft);
	}
	.who {
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.scrim {
		display: none;
	}
	@media (max-width: 760px) {
		nav {
			position: fixed;
			inset: 0 auto 0 0;
			z-index: 50;
			width: 280px;
			transform: translateX(-100%);
			transition: transform 0.2s ease;
		}
		/* Shadow only when open: closed, it would bleed onto the page's left edge. */
		nav.open {
			transform: none;
			box-shadow: var(--shadow);
		}
		.scrim {
			display: block;
			position: fixed;
			inset: 0;
			z-index: 40;
			border: 0;
			background: rgb(0 0 0 / 0.35);
		}
		.actions {
			opacity: 1;
		}
		li .count {
			visibility: visible;
			margin-right: 6px;
		}
	}
</style>
