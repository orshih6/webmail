<script lang="ts">
import { goto } from '$app/navigation';
import { page } from '$app/state';
import { api, qs } from '$lib/api/client';
import type { Contact } from '$lib/api/types/Contact';
import type { Identity } from '$lib/api/types/Identity';
import type { IdentityInput } from '$lib/api/types/IdentityInput';
import type { Prefs } from '$lib/api/types/Prefs';
import Icon from '$lib/components/Icon.svelte';
import Skeleton from '$lib/components/Skeleton.svelte';
import { app, applyTheme, brand, fail, pageTitle, toast } from '$lib/state.svelte';

type Tab = 'general' | 'identities' | 'contacts' | 'about';
const tab = $derived((page.url.searchParams.get('tab') ?? 'general') as Tab);
const tabs: [Tab, string][] = [
	['general', 'General'],
	['identities', 'Identities'],
	['contacts', 'Contacts'],
	['about', 'About']
];

// ---- General ----------------------------------------------------------------------------
async function savePrefs(patch: Partial<Prefs>) {
	const before = { ...app.prefs };
	Object.assign(app.prefs, patch);
	if (patch.theme) applyTheme(patch.theme);
	try {
		app.prefs = await api<Prefs>('POST', '/prefs', app.prefs);
		app.changed++;
	} catch (e) {
		app.prefs = before;
		applyTheme(before.theme);
		fail(e);
	}
}

// ---- Identities -------------------------------------------------------------------------
let identities = $state<Identity[]>([]);
let editing = $state<IdentityInput | null>(null);

let identitiesLoaded = $state(false);
async function loadIdentities() {
	try {
		identities = await api<Identity[]>('GET', '/identities');
		identitiesLoaded = true;
	} catch (e) {
		fail(e);
	}
}

function edit(i?: Identity) {
	editing = i
		? {
				id: i.id,
				name: i.name,
				reply_to: i.reply_to,
				signature: i.signature,
				is_default: i.is_default
			}
		: { id: null, name: '', reply_to: '', signature: '', is_default: false };
}

async function saveIdentity(e: SubmitEvent) {
	e.preventDefault();
	if (!editing) return;
	try {
		identities = await api<Identity[]>('POST', '/identities', editing);
		editing = null;
		toast('Identity saved');
	} catch (err) {
		fail(err);
	}
}

async function deleteIdentity(i: Identity) {
	if (i.id === null || !confirm(`Delete the identity “${i.name || i.email}”?`)) return;
	try {
		identities = await api<Identity[]>('DELETE', `/identities/${i.id}`);
	} catch (e) {
		fail(e);
	}
}

// ---- Contacts ---------------------------------------------------------------------------
let contacts = $state<Contact[]>([]);
let filter = $state('');
let newName = $state('');
let newEmail = $state('');
let editingContact = $state<number | null>(null);
let editName = $state('');
let editEmail = $state('');

const shortcuts: [string, string][] = [
	['c', 'Compose'],
	['j / ↓', 'Next message'],
	['k / ↑', 'Previous message'],
	['r', 'Reply'],
	['a', 'Reply all'],
	['f', 'Forward'],
	['s', 'Star / unstar'],
	['u', 'Mark unread and close'],
	['x', 'Select'],
	['#  or  Delete', 'Delete'],
	['!', 'Junk'],
	['/', 'Search'],
	['Esc', 'Close / clear selection'],
	['⌘/Ctrl + Enter', 'Send (while writing)'],
	['⌘/Ctrl + S', 'Save draft (while writing)'],
	['⌘/Ctrl + K', 'Insert link (rich text)']
];

const shown = $derived(
	contacts.filter((c) => `${c.name} ${c.email}`.toLowerCase().includes(filter.trim().toLowerCase()))
);

let contactsLoaded = $state(false);
async function loadContacts() {
	try {
		contacts = await api<Contact[]>('GET', '/contacts');
		contactsLoaded = true;
	} catch (e) {
		fail(e);
	}
}

async function addContact(e: SubmitEvent) {
	e.preventDefault();
	try {
		await api<Contact>('POST', '/contacts', { id: null, name: newName, email: newEmail });
		newName = newEmail = '';
		await loadContacts();
	} catch (err) {
		fail(err);
	}
}

async function saveContact(c: Contact) {
	try {
		await api<Contact>('POST', '/contacts', { id: c.id, name: editName, email: editEmail });
		editingContact = null;
		await loadContacts();
	} catch (e) {
		fail(e);
	}
}

async function deleteContact(c: Contact) {
	if (!confirm(`Delete ${c.name || c.email} from your contacts?`)) return;
	contacts = contacts.filter((x) => x.id !== c.id);
	try {
		await api('DELETE', `/contacts/${c.id}`);
	} catch (e) {
		fail(e);
		loadContacts();
	}
}

$effect(() => {
	if (tab === 'identities') loadIdentities();
	if (tab === 'contacts') loadContacts();
});
</script>

<svelte:head><title>{pageTitle('Settings')}</title></svelte:head>

<section class="settings">
	<div class="bar">
		<button class="icon-btn menu" aria-label="Folders" onclick={() => (app.drawer = true)}><Icon name="menu" /></button>
		<h1>Settings</h1>
	</div>
	<nav class="tabs" aria-label="Settings sections">
		{#each tabs as [id, label] (id)}
			<a href="/mail/settings?{qs({ tab: id })}" class:active={tab === id} aria-current={tab === id ? 'page' : undefined}>{label}</a>
		{/each}
	</nav>

	<div class="body">
		{#if tab === 'general'}
			<div class="group">
				<h2>Appearance</h2>
				<div class="choice" role="radiogroup" aria-label="Theme">
					{#each [['system', 'Match system'], ['light', 'Light'], ['dark', 'Dark']] as [v, label] (v)}
						<label>
							<input type="radio" name="theme" checked={app.prefs.theme === v} onchange={() => savePrefs({ theme: v as Prefs['theme'] })} />
							{label}
						</label>
					{/each}
				</div>
			</div>
			<div class="group">
				<h2>Message list</h2>
				<label class="line">
					<span>Messages per page</span>
					<select class="field small" value={String(app.prefs.page_size)} onchange={(e) => savePrefs({ page_size: Number(e.currentTarget.value) })}>
						{#each [25, 50, 100, 200] as n (n)}<option value={String(n)}>{n}</option>{/each}
					</select>
				</label>
				<label class="line">
					<input type="checkbox" checked={app.prefs.conversations} onchange={(e) => savePrefs({ conversations: e.currentTarget.checked })} />
					<span>Group messages into conversations</span>
				</label>
			</div>
			<div class="group">
				<h2>Writing</h2>
				<label class="line">
					<input type="checkbox" checked={app.prefs.compose_html} onchange={(e) => savePrefs({ compose_html: e.currentTarget.checked })} />
					<span>Write new messages in rich text (bold, lists, links, images)</span>
				</label>
			</div>
			<div class="group">
				<h2>Privacy</h2>
				<p class="hint">Remote images can tell the sender when and where you opened a message.</p>
				<div class="choice column" role="radiogroup" aria-label="Load remote images">
					{#each [['never', 'Always ask before showing remote images'], ['contacts', 'Show them for people in my contacts'], ['always', 'Always show remote images']] as [v, label] (v)}
						<label>
							<input type="radio" name="images" checked={app.prefs.remote_images === v} onchange={() => savePrefs({ remote_images: v as Prefs['remote_images'] })} />
							{label}
						</label>
					{/each}
				</div>
			</div>
		{:else if tab === 'identities'}
			<p class="hint">
				Identities change the name people see, where replies go, and your signature. Mail is always
				sent from <strong>{app.email}</strong>.
			</p>
			{#if !identitiesLoaded}<div aria-busy="true"><Skeleton kind="lines" count={2} /></div>{/if}
			<ul class="cards">
				{#each identities as i (i.id ?? 'builtin')}
					<li>
						<div class="card-main">
							<strong>{i.name || 'No name'}</strong>
							{#if i.is_default}<span class="badge">Default</span>{/if}
							<div class="sub">{i.email}{i.reply_to ? ` · replies to ${i.reply_to}` : ''}</div>
							{#if i.signature}<pre class="sig">{i.signature}</pre>{/if}
						</div>
						<button class="btn" onclick={() => edit(i)}>Edit</button>
						{#if i.id !== null && identities.length > 1}
							<button class="icon-btn" aria-label="Delete identity" onclick={() => deleteIdentity(i)}><Icon name="trash" /></button>
						{/if}
					</li>
				{/each}
			</ul>
			{#if editing}
				<form class="editor" onsubmit={saveIdentity}>
					<h2>{editing.id === null ? 'New identity' : 'Edit identity'}</h2>
					<label><span>Your name</span><input class="field" bind:value={editing.name} placeholder="Alice Smith" /></label>
					<label><span>Reply-To (optional)</span><input class="field" type="email" bind:value={editing.reply_to} placeholder="replies@example.com" /></label>
					<label><span>Signature</span><textarea class="field" rows="4" bind:value={editing.signature}></textarea></label>
					<label class="line"><input type="checkbox" bind:checked={editing.is_default} /> <span>Use by default</span></label>
					<div class="row-actions">
						<button class="btn primary" type="submit">Save</button>
						<button class="btn" type="button" onclick={() => (editing = null)}>Cancel</button>
					</div>
				</form>
			{:else}
				<button class="btn" onclick={() => edit()}>+ Add identity</button>
			{/if}
		{:else if tab === 'about'}
			<div class="about">
				<img src="/icons/logo.svg" alt="" width="56" height="56" />
				<div>
					<h2 class="name">{brand.app_name}</h2>
					<p class="hint">Version {brand.version || '—'}</p>
				</div>
			</div>
			<p>
				Free software under the
				<a href="https://www.gnu.org/licenses/agpl-3.0.html" rel="noopener" target="_blank"
					>GNU Affero General Public License v3.0</a
				>. You can get the source code of the version running here:
				<a href={brand.source_url} rel="noopener" target="_blank">{brand.source_url}</a>
			</p>
			<div class="group">
				<h2>Keyboard shortcuts</h2>
				<dl class="keys">
					{#each shortcuts as [k, what] (k)}
						<dt><kbd>{k}</kbd></dt>
						<dd>{what}</dd>
					{/each}
				</dl>
			</div>
		{:else}
			<form class="add" onsubmit={addContact}>
				<input class="field" placeholder="Name" bind:value={newName} aria-label="Name" />
				<input class="field" type="email" placeholder="email@example.com" required bind:value={newEmail} aria-label="Email" />
				<button class="btn primary" type="submit">Add</button>
			</form>
			{#if contacts.length > 8}
				<input class="field filter" type="search" placeholder="Filter contacts" bind:value={filter} aria-label="Filter contacts" />
			{/if}
			{#if !contactsLoaded}<div aria-busy="true"><Skeleton kind="lines" count={4} /></div>{/if}
			<ul class="contacts">
				{#each shown as c (c.id)}
					<li>
						{#if editingContact === c.id}
							<input class="field" bind:value={editName} aria-label="Name" />
							<input class="field" type="email" bind:value={editEmail} aria-label="Email" />
							<button class="btn primary" onclick={() => saveContact(c)}>Save</button>
							<button class="btn" onclick={() => (editingContact = null)}>Cancel</button>
						{:else}
							<div class="avatar" aria-hidden="true">{(c.name || c.email).charAt(0).toUpperCase()}</div>
							<div class="card-main">
								<strong>{c.name || c.email}</strong>
								{#if c.name}<div class="sub">{c.email}</div>{/if}
							</div>
							<button class="icon-btn" title="Write to {c.email}" aria-label="Write" onclick={() => goto(`/mail/compose?${qs({ to: c.name ? `${c.name} <${c.email}>` : c.email })}`)}><Icon name="pen" /></button>
							<button
								class="icon-btn"
								aria-label="Edit contact"
								onclick={() => {
									editingContact = c.id;
									editName = c.name;
									editEmail = c.email;
								}}><Icon name="draft" /></button
							>
							<button class="icon-btn" aria-label="Delete contact" onclick={() => deleteContact(c)}><Icon name="trash" /></button>
						{/if}
					</li>
				{:else}
					{#if contactsLoaded}<li class="empty">No contacts yet. People you write to are suggested automatically while you type.</li>{/if}
				{/each}
			</ul>
		{/if}
	</div>
</section>

<style>
	.settings {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 52px;
		padding: 0 18px;
		border-bottom: 1px solid var(--border);
		flex: none;
	}
	h1 {
		margin: 0;
		font-size: 16px;
	}
	.menu {
		display: none;
	}
	.tabs {
		display: flex;
		gap: 4px;
		padding: 8px 14px 0;
		border-bottom: 1px solid var(--border);
	}
	.tabs a {
		padding: 8px 12px;
		border-bottom: 2px solid transparent;
		color: var(--text-soft);
		text-decoration: none;
		font-weight: 500;
	}
	.tabs a.active {
		color: var(--text);
		border-color: var(--accent);
	}
	.body {
		flex: 1;
		overflow-y: auto;
		padding: 20px 24px 40px;
		max-width: 720px;
	}
	.group {
		margin-bottom: 28px;
	}
	h2 {
		margin: 0 0 10px;
		font-size: 14px;
		font-weight: 650;
	}
	.hint {
		margin: 0 0 12px;
		color: var(--text-soft);
	}
	.choice {
		display: flex;
		gap: 16px;
		flex-wrap: wrap;
	}
	.choice.column {
		flex-direction: column;
		gap: 8px;
	}
	.choice label,
	.line {
		display: flex;
		align-items: center;
		gap: 8px;
		cursor: pointer;
	}
	.line {
		margin-bottom: 10px;
	}
	.line > span:first-child {
		min-width: 140px;
	}
	.field.small {
		width: auto;
		height: 32px;
	}
	.about {
		display: flex;
		align-items: center;
		gap: 14px;
		margin-bottom: 16px;
	}
	.about .name {
		margin: 0;
		font-size: 20px;
	}
	.about .hint {
		margin: 2px 0 0;
	}
	.keys {
		display: grid;
		grid-template-columns: max-content 1fr;
		gap: 8px 16px;
		margin: 0;
	}
	.keys dt,
	.keys dd {
		margin: 0;
	}
	kbd {
		padding: 1px 7px;
		border: 1px solid var(--border);
		border-bottom-width: 2px;
		border-radius: 6px;
		background: var(--bg-soft);
		font: 12.5px var(--mono);
	}
	.cards,
	.contacts {
		list-style: none;
		margin: 0 0 16px;
		padding: 0;
	}
	.cards li,
	.contacts li {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 12px 0;
		border-bottom: 1px solid var(--border);
	}
	.cards li {
		align-items: flex-start;
	}
	.card-main {
		flex: 1;
		min-width: 0;
	}
	.sub {
		color: var(--text-soft);
		font-size: 13px;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.badge {
		margin-left: 6px;
		padding: 1px 7px;
		border-radius: 10px;
		background: var(--accent-soft);
		color: var(--accent);
		font-size: 11.5px;
		font-weight: 600;
	}
	.sig {
		margin: 8px 0 0;
		font: inherit;
		font-size: 13px;
		color: var(--text-soft);
		white-space: pre-wrap;
	}
	.editor {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 16px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-soft);
	}
	.editor label:not(.line) {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		color: var(--text-soft);
	}
	textarea.field {
		height: auto;
		padding: 8px 10px;
		font: inherit;
		resize: vertical;
	}
	.row-actions {
		display: flex;
		gap: 8px;
	}
	.add {
		display: flex;
		gap: 8px;
		margin-bottom: 16px;
	}
	.filter {
		margin-bottom: 8px;
	}
	.contacts .field {
		height: 32px;
	}
	.avatar {
		flex: none;
		width: 34px;
		height: 34px;
		display: grid;
		place-items: center;
		border-radius: 50%;
		background: var(--accent-soft);
		color: var(--accent);
		font-weight: 700;
	}
	.empty {
		color: var(--text-faint);
		border: 0 !important;
	}
	@media (max-width: 760px) {
		.menu {
			display: inline-grid;
		}
		.bar {
			padding: 0 10px;
		}
		.body {
			padding: 16px;
		}
		.add {
			flex-wrap: wrap;
		}
	}
</style>
