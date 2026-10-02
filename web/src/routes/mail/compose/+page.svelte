<script lang="ts">
import { beforeNavigate, goto } from '$app/navigation';
import { page } from '$app/state';
import { api, qs } from '$lib/api/client';
import type { Address } from '$lib/api/types/Address';
import type { Attachment } from '$lib/api/types/Attachment';
import type { ComposeRequest } from '$lib/api/types/ComposeRequest';
import type { DraftSaved } from '$lib/api/types/DraftSaved';
import type { Identity } from '$lib/api/types/Identity';
import type { MessageDetail } from '$lib/api/types/MessageDetail';
import type { MessageRef } from '$lib/api/types/MessageRef';
import type { Upload } from '$lib/api/types/Upload';
import Icon from '$lib/components/Icon.svelte';
import RecipientInput from '$lib/components/RecipientInput.svelte';
import RichEditor, { textToHtml } from '$lib/components/RichEditor.svelte';
import Skeleton from '$lib/components/Skeleton.svelte';
import Spinner from '$lib/components/Spinner.svelte';
import { display, folderUrl, fullList, longDate, size } from '$lib/format';
import {
	app,
	type ComposeBackup,
	clearComposeBackup,
	fail,
	loadComposeBackup,
	pageTitle,
	saveComposeBackup,
	toast
} from '$lib/state.svelte';

type Mode = 'new' | 'reply' | 'all' | 'forward' | 'draft';
// State, not constants: restoring a backed-up message can turn a new compose into a reply.
let mode = $state((page.url.searchParams.get('mode') ?? 'new') as Mode);
const srcFolder = page.url.searchParams.get('f');
const srcUid = Number(page.url.searchParams.get('m')) || null;
let source = $state<MessageRef | null>(
	srcFolder && srcUid ? { folder: srcFolder, uid: srcUid } : null
);

let to = $state(page.url.searchParams.get('to') ?? '');
let cc = $state('');
let bcc = $state('');
let showCc = $state(false);
let subject = $state('');
let text = $state('');
let uploads = $state<Upload[]>([]);
/** Attachments carried over from the source message (forward / draft). */
let kept = $state<Attachment[]>([]);
let inReplyTo = $state<string | null>(null);
let references = $state<string[]>([]);
let draft = $state<MessageRef | null>(null);

let ready = $state(false);
let identities = $state<Identity[]>([]);
let identityId = $state<string>('');
/** The signature block currently in the body, so switching identity can swap it. */
let sigInBody = '';
let sending = $state(false);
let saving = $state(false);
let uploading = $state(0);
let dirty = $state(false);
let savedAt = $state<Date | null>(null);
let bodyEl = $state<HTMLTextAreaElement>();
/** Rich-text mode: `html` is the editor's content, `text` is unused until switching back. */
let rich = $state(false);
let html = $state('');
let editor = $state<ReturnType<typeof RichEditor>>();
let toEl = $state<HTMLInputElement>();

const prefix = (p: string, s: string) =>
	s.toLowerCase().startsWith(p.toLowerCase()) ? s : `${p} ${s}`;
const notMe = (a: Address) => a.email.toLowerCase() !== app.email.toLowerCase();
const quote = (t: string) =>
	t
		.trimEnd()
		.split('\n')
		.map((l) => `> ${l}`)
		.join('\n');

/** Signature as it sits in the body: after a blank line, with the "-- " delimiter. */
function sigBlock(i: Identity | undefined) {
	const sig = i?.signature.trim();
	if (!sig) return '';
	return `\n\n${/^--\s*\n/.test(sig) ? sig : `-- \n${sig}`}`;
}

function setIdentity(id: string) {
	identityId = id;
	const next = sigBlock(identities.find((i) => String(i.id) === id));
	if (rich) {
		// Same block, as the editor holds it.
		const asHtml = (b: string) =>
			b ? textToHtml(b.replace(/^\n\n/, '')).replace(/<br>$/, '') : '';
		const was = asHtml(sigInBody);
		const now = asHtml(next);
		html =
			was && html.includes(was) ? html.replace(was, now) : (now ? `<br><br>${now}` : '') + html;
	} else {
		text = sigInBody && text.includes(sigInBody) ? text.replace(sigInBody, next) : next + text;
	}
	sigInBody = next;
}

/** The body of the sanitized document the API returns for display. */
function bodyOf(doc: string) {
	return new DOMParser().parseFromString(doc, 'text/html').body.innerHTML;
}

async function uploadFile(file: File): Promise<Upload> {
	const fd = new FormData();
	fd.append('file', file, file.name);
	return api<Upload>('POST', '/uploads', fd);
}

async function uploadInline(file: File) {
	try {
		const u = await uploadFile(file);
		dirty = true;
		return { id: u.id, url: `/api/uploads/${encodeURIComponent(u.id)}` };
	} catch (e) {
		fail(e);
		return null;
	}
}

/** A reopened draft shows its images as data: URIs; turn them back into uploads so they are
 *  sent as proper inline parts. Decoded by hand: the CSP does not allow fetch(data:). */
async function rehostDataImages() {
	const doc = document.implementation.createHTMLDocument('');
	doc.body.innerHTML = html;
	const imgs = [...doc.querySelectorAll<HTMLImageElement>('img[src^="data:image/"]')];
	if (!imgs.length) return;
	for (const img of imgs) {
		const m = /^data:(image\/(?:png|jpeg|gif|webp));base64,(.*)$/.exec(
			img.getAttribute('src') ?? ''
		);
		if (!m) continue;
		const bytes = Uint8Array.from(atob(m[2]), (c) => c.charCodeAt(0));
		const up = await uploadInline(new File([bytes], 'image', { type: m[1] }));
		if (up) {
			img.setAttribute('src', up.url);
			img.setAttribute('data-upload', up.id);
		}
	}
	html = doc.body.innerHTML;
}

function toggleRich() {
	if (rich) {
		if (
			html.includes('<img') &&
			!confirm('Switch to plain text? Formatting and inline images will be removed.')
		)
			return;
		text = editor?.text() ?? '';
		rich = false;
	} else {
		html = textToHtml(text);
		rich = true;
	}
	dirty = true;
}

async function prefill(d: MessageDetail) {
	const body = d.text ?? '';
	const ids = [...d.references, ...(d.message_id ? [d.message_id] : [])];
	if (mode === 'reply' || mode === 'all') {
		const replyTo = d.reply_to.length ? d.reply_to : d.from ? [d.from] : [];
		to = fullList(replyTo);
		if (mode === 'all') {
			const seen = new Set(replyTo.map((a) => a.email.toLowerCase()));
			const others = [...d.to, ...d.cc].filter((a) => notMe(a) && !seen.has(a.email.toLowerCase()));
			cc = fullList(others);
			showCc = others.length > 0;
		}
		subject = prefix('Re:', d.subject);
		inReplyTo = d.message_id;
		references = ids;
		text = `\n\nOn ${longDate(d.date)}, ${display(d.from)} wrote:\n${quote(body)}\n`;
	} else if (mode === 'forward') {
		subject = prefix('Fwd:', d.subject);
		kept = d.attachments;
		text = [
			'',
			'',
			'---------- Forwarded message ----------',
			`From: ${d.from ? fullList([d.from]) : ''}`,
			`Date: ${longDate(d.date)}`,
			`Subject: ${d.subject}`,
			`To: ${fullList(d.to)}`,
			...(d.cc.length ? [`Cc: ${fullList(d.cc)}`] : []),
			'',
			body
		].join('\n');
	} else if (mode === 'draft') {
		to = fullList(d.to);
		cc = fullList(d.cc);
		bcc = fullList(d.bcc);
		showCc = !!(cc || bcc);
		subject = d.subject;
		text = body.trimEnd(); // the stored copy gains a final newline on every save
		if (d.html) {
			rich = true;
			html = bodyOf(d.html);
		}
		kept = d.attachments;
		inReplyTo = d.in_reply_to;
		references = d.references;
		draft = source;
	}
}

// Load identities and the source message, then place the signature above any quote.
$effect(() => {
	(async () => {
		const ids = api<Identity[]>('GET', '/identities').catch(() => [] as Identity[]);
		if (source && mode !== 'new') {
			try {
				await prefill(
					await api<MessageDetail>(
						'GET',
						`/message?${qs({ folder: source.folder, uid: source.uid })}`
					)
				);
			} catch (e) {
				fail(e);
			}
		}
		identities = await ids;
		const def = identities[0];
		if (def) {
			identityId = String(def.id ?? '');
			// A draft already contains whatever signature it was written with.
			if (mode !== 'draft') setIdentity(identityId);
		}
		if (mode !== 'draft' && app.prefs.compose_html) {
			html = textToHtml(text);
			rich = true;
		}
		if (rich) rehostDataImages().catch(fail);
		// A message that never reached the server last time? Offer it back.
		const b = mode === 'new' && !to ? loadComposeBackup() : null;
		if (b && Date.now() - b.savedAt < 30 * 24 * 3600 * 1000) offer = b;
		ready = true;
		queueMicrotask(() => {
			if (mode === 'forward' || !to) return toEl?.focus();
			if (rich) return editor?.focus();
			bodyEl?.focus();
			bodyEl?.setSelectionRange(0, 0);
		});
	})();
});

// ---- Local backup ------------------------------------------------------------------------
let offer = $state<ComposeBackup | null>(null);

// Keep a copy in this browser while there are changes the server doesn't have yet.
$effect(() => {
	if (!ready || !dirty || offer) return;
	const b: ComposeBackup = {
		savedAt: Date.now(),
		mode,
		source,
		draft,
		to,
		cc,
		bcc,
		subject,
		text,
		html,
		rich,
		inReplyTo,
		references: [...references],
		identityId,
		hadAttachments: uploads.length + kept.length > 0
	};
	const t = setTimeout(() => saveComposeBackup(b), 800);
	return () => clearTimeout(t);
});

function restore() {
	const b = offer;
	if (!b) return;
	mode = b.mode as Mode;
	source = b.source;
	draft = b.draft;
	to = b.to;
	cc = b.cc;
	bcc = b.bcc;
	showCc = !!(b.cc || b.bcc);
	subject = b.subject;
	inReplyTo = b.inReplyTo;
	references = b.references;
	if (b.identityId) identityId = b.identityId;
	sigInBody = ''; // the restored body already has whatever signature it had
	rich = b.rich;
	if (b.rich) html = b.html;
	else text = b.text;
	offer = null;
	dirty = true;
	if (b.hadAttachments) toast('Attachments were not kept — please add them again');
}

function dismissOffer() {
	clearComposeBackup();
	offer = null;
}

/** Editor HTML → what is sent: uploaded images become cid: references to inline parts. */
function richBody() {
	const doc = document.implementation.createHTMLDocument('');
	doc.body.innerHTML = html;
	const inline: string[] = [];
	for (const img of doc.querySelectorAll('img[data-upload]')) {
		const id = img.getAttribute('data-upload') ?? '';
		img.setAttribute('src', `cid:${id}`);
		img.removeAttribute('data-upload');
		inline.push(id);
	}
	return { html: doc.body.innerHTML, inline };
}

function request(): ComposeRequest {
	// Once saved, the draft holds every attachment; before that, a forward keeps the original's.
	const keepSource = draft ?? (mode === 'forward' ? source : null);
	const body = rich ? richBody() : null;
	return {
		to,
		cc,
		bcc,
		subject,
		text: rich ? (editor?.text() ?? '') : text,
		html: body?.html ?? null,
		inline_uploads: body?.inline ?? [],
		uploads: uploads.map((u) => u.id),
		keep:
			keepSource && kept.length ? { source: keepSource, indices: kept.map((a) => a.index) } : null,
		in_reply_to: inReplyTo,
		references,
		reply_of: (mode === 'reply' || mode === 'all') && source ? source : null,
		forward_of: mode === 'forward' && source ? source : null,
		draft,
		identity: identityId ? Number(identityId) : null
	};
}

async function send() {
	if (sending) return;
	if (!to.trim() && !cc.trim() && !bcc.trim()) {
		toast('Add at least one recipient', 'error');
		toEl?.focus();
		return;
	}
	if (!subject.trim() && !confirm('Send without a subject?')) return;
	sending = true;
	try {
		await api('POST', '/send', request());
		dirty = false;
		clearComposeBackup();
		toast('Message sent');
		app.changed++;
		close();
	} catch (e) {
		fail(e);
	} finally {
		sending = false;
	}
}

/** True once the server has the draft. */
async function saveDraft(quiet = false): Promise<boolean> {
	if (saving || sending) return false;
	saving = true;
	try {
		const res = await api<DraftSaved>('POST', '/drafts', request());
		// The new draft now holds every attachment, kept ones first, then uploads (the
		// server's order), and the server has dropped the uploads. Point at the draft.
		if (res.draft) {
			kept = [...kept, ...uploads].map((a, index) => ({
				index,
				filename: a.filename,
				content_type: a.content_type,
				size: a.size
			}));
			uploads = [];
		}
		draft = res.draft;
		dirty = false;
		clearComposeBackup();
		savedAt = new Date();
		if (!quiet) toast('Draft saved');
		return true;
	} catch (e) {
		if (!quiet) fail(e);
		return false;
	} finally {
		saving = false;
	}
}

// Autosave every 30s while there are unsaved changes.
$effect(() => {
	const t = setInterval(() => dirty && ready && saveDraft(true), 30_000);
	return () => clearInterval(t);
});

let leaving = false;
beforeNavigate(({ cancel, to: dest }) => {
	if (!dirty || leaving || sending) return;
	cancel();
	leaving = true;
	saveDraft(true).then((ok) => {
		toast(
			ok
				? 'Saved to Drafts'
				: "Couldn't save the draft — it's kept in this browser and will be offered next time you compose",
			ok ? 'info' : 'error'
		);
		if (dest) goto(dest.url);
	});
});

function close() {
	leaving = true;
	goto(
		source && mode !== 'draft'
			? folderUrl(source.folder, { m: source.uid })
			: folderUrl(srcFolder ?? 'INBOX')
	);
}

async function discard() {
	if (dirty && !confirm('Discard this message?')) return;
	dirty = false;
	clearComposeBackup();
	for (const u of uploads) api('DELETE', `/uploads/${encodeURIComponent(u.id)}`).catch(() => {});
	if (draft) {
		try {
			await api('POST', '/messages/delete', { folder: draft.folder, uids: [draft.uid] });
			app.changed++;
		} catch {}
	}
	close();
}

async function addFiles(files: FileList | null) {
	for (const file of files ?? []) {
		if (file.size > 25 * 1024 * 1024) {
			toast(`${file.name} is larger than 25 MB`, 'error');
			continue;
		}
		uploading++;
		try {
			uploads.push(await uploadFile(file));
			dirty = true;
		} catch (e) {
			fail(e);
		} finally {
			uploading--;
		}
	}
}

function removeUpload(u: Upload) {
	uploads = uploads.filter((x) => x.id !== u.id);
	dirty = true;
	api('DELETE', `/uploads/${encodeURIComponent(u.id)}`).catch(() => {});
}

let dragging = $state(false);
function ondrop(e: DragEvent) {
	e.preventDefault();
	dragging = false;
	addFiles(e.dataTransfer?.files ?? null);
}

function onkey(e: KeyboardEvent) {
	if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
		e.preventDefault();
		send();
	} else if ((e.metaKey || e.ctrlKey) && e.key === 's') {
		e.preventDefault();
		saveDraft();
	}
}

const title = $derived(
	{ new: 'New message', reply: 'Reply', all: 'Reply all', forward: 'Forward', draft: 'Draft' }[mode]
);
</script>

<svelte:window onkeydown={onkey} />
<svelte:head><title>{pageTitle(subject || title)}</title></svelte:head>

<section
	class="compose"
	class:dragging
	ondragover={(e) => {
		e.preventDefault();
		dragging = true;
	}}
	ondragleave={() => (dragging = false)}
	{ondrop}
	aria-label={title}
>
	<div class="bar">
		<button class="icon-btn" aria-label="Close" onclick={close}><Icon name="back" /></button>
		<h1>{title}</h1>
		<span class="status">
			{#if saving}Saving…{:else if savedAt}Saved {savedAt.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}{/if}
		</span>
		<button class="icon-btn" title="Discard" aria-label="Discard" onclick={discard}><Icon name="trash" /></button>
	</div>

	{#if offer}
		<div class="restore" role="status">
			<span>
				You have an unsent message{offer.subject ? ` “${offer.subject}”` : ''} from
				{new Date(offer.savedAt).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}.
			</span>
			<button class="btn primary" onclick={restore}>Restore it</button>
			<button class="btn" onclick={dismissOffer}>Discard</button>
		</div>
	{/if}
	{#if !ready}
		<div class="loading" aria-busy="true">
			<span class="sr-only" role="status">Loading…</span>
			<Skeleton kind="form" count={2} />
			<div class="sk-body"><Skeleton kind="message" /></div>
		</div>
	{:else}
		<div class="fields" oninput={() => (dirty = true)}>
			{#if identities.length > 1}
				<label class="row">
					<span>From</span>
					<select class="from" value={identityId} onchange={(e) => setIdentity(e.currentTarget.value)}>
						{#each identities as i (i.id)}
							<option value={String(i.id ?? '')}>{i.name ? `${i.name} <${i.email}>` : i.email}</option>
						{/each}
					</select>
				</label>
			{/if}
			<label class="row">
				<span>To</span>
				<RecipientInput bind:value={to} bind:input={toEl} label="To" />
				{#if !showCc}<button class="link" type="button" onclick={() => (showCc = true)}>Cc / Bcc</button>{/if}
			</label>
			{#if showCc}
				<label class="row"><span>Cc</span><RecipientInput bind:value={cc} label="Cc" /></label>
				<label class="row"><span>Bcc</span><RecipientInput bind:value={bcc} label="Bcc" /></label>
			{/if}
			<label class="row"><span>Subject</span><input bind:value={subject} /></label>
			{#if rich}
				<RichEditor bind:this={editor} bind:html onimage={uploadInline} oninput={() => (dirty = true)} />
			{:else}
				<textarea bind:this={bodyEl} bind:value={text} aria-label="Message" placeholder="Write your message…"></textarea>
			{/if}
		</div>

		{#if kept.length || uploads.length || uploading}
			<ul class="files">
				{#each kept as a (a.index)}
					<li>
						<Icon name="clip" size={14} /><span class="fname">{a.filename}</span><span class="fsize">{size(a.size)}</span>
						<button
							aria-label="Remove {a.filename}"
							onclick={() => {
								kept = kept.filter((k) => k.index !== a.index);
								dirty = true;
							}}><Icon name="x" size={14} /></button
						>
					</li>
				{/each}
				{#each uploads as u (u.id)}
					<li>
						<Icon name="clip" size={14} /><span class="fname">{u.filename}</span><span class="fsize">{size(u.size)}</span>
						<button aria-label="Remove {u.filename}" onclick={() => removeUpload(u)}><Icon name="x" size={14} /></button>
					</li>
				{/each}
				{#if uploading}<li class="pending">Uploading {uploading}…</li>{/if}
			</ul>
		{/if}

		<div class="actions">
			<button class="btn primary" onclick={send} disabled={sending || uploading > 0}>
				{#if sending}<Spinner />{:else}<Icon name="send" size={16} />{/if}
				{sending ? 'Sending…' : 'Send'}
			</button>
			<label class="btn attach">
				<Icon name="clip" size={16} /> Attach
				<input type="file" multiple class="sr-only" onchange={(e) => addFiles(e.currentTarget.files)} />
			</label>
			<button class="btn" onclick={() => saveDraft()} disabled={saving}>Save draft</button>
			<button class="btn format" onclick={toggleRich} aria-pressed={rich} title="Switch between plain and rich text">
				{rich ? 'Plain text' : 'Rich text'}
			</button>
			<span class="hint">⌘/Ctrl + Enter to send</span>
		</div>
	{/if}
</section>

<style>
	.compose {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-width: 0;
		max-width: 900px;
	}
	.compose.dragging {
		outline: 2px dashed var(--accent);
		outline-offset: -8px;
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 52px;
		padding: 0 10px;
		border-bottom: 1px solid var(--border);
		flex: none;
	}
	h1 {
		margin: 0;
		font-size: 16px;
		flex: 1;
	}
	.status {
		font-size: 12px;
		color: var(--text-faint);
	}
	.restore {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px 10px;
		padding: 10px 18px;
		border-bottom: 1px solid var(--border);
		background: var(--accent-soft);
	}
	.restore span {
		flex: 1;
		min-width: 200px;
	}
	.loading {
		flex: 1;
	}
	/* The body placeholder without the reader's title row. */
	.sk-body :global(.title),
	.sk-body :global(.meta) {
		display: none;
	}
	.fields {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 44px;
		padding: 0 18px;
		border-bottom: 1px solid var(--border);
	}
	.row span {
		width: 56px;
		flex: none;
		color: var(--text-faint);
		font-size: 13px;
	}
	.row input {
		flex: 1;
		min-width: 0;
		height: 42px;
		border: 0;
		outline: none;
		background: none;
	}
	.from {
		flex: 1;
		min-width: 0;
		height: 42px;
		border: 0;
		background: none;
		outline: none;
	}
	.link {
		border: 0;
		background: none;
		color: var(--accent);
		font-size: 13px;
		font-weight: 600;
	}
	textarea {
		flex: 1;
		min-height: 240px;
		padding: 16px 18px;
		border: 0;
		outline: none;
		resize: none;
		background: none;
		font: inherit;
		line-height: 1.55;
	}
	.files {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		list-style: none;
		margin: 0;
		padding: 10px 18px;
		border-top: 1px solid var(--border);
	}
	.files li {
		display: flex;
		align-items: center;
		gap: 6px;
		max-width: 260px;
		padding: 5px 6px 5px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 13px;
	}
	.files li.pending {
		color: var(--text-faint);
		padding-right: 10px;
	}
	.files button {
		display: grid;
		place-items: center;
		border: 0;
		background: none;
		color: var(--text-faint);
		padding: 2px;
	}
	.fname {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.fsize {
		color: var(--text-faint);
		flex: none;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 12px 18px;
		border-top: 1px solid var(--border);
		flex: none;
	}
	.attach {
		cursor: pointer;
	}
	.hint {
		margin-left: auto;
		font-size: 12px;
		color: var(--text-faint);
	}
	@media (max-width: 760px) {
		.hint {
			display: none;
		}
	}
</style>
