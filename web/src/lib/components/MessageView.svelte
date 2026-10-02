<script lang="ts">
import { api, qs } from '$lib/api/client';
import type { MessageDetail } from '$lib/api/types/MessageDetail';
import { display, fullList, longDate, size } from '$lib/format';
import { fail, toast } from '$lib/state.svelte';
import Icon from './Icon.svelte';

let {
	folder,
	uid,
	onloaded
}: { folder: string; uid: number; onloaded?: (d: MessageDetail) => void } = $props();

let detail = $state<MessageDetail | null>(null);
let loading = $state(true);
let images = $state(false);

$effect(() => {
	const key = { folder, uid, images };
	let cancelled = false;
	loading = true;
	api<MessageDetail>(
		'GET',
		`/message?${qs({ folder: key.folder, uid: key.uid, images: key.images ? 1 : undefined })}`
	)
		.then((d) => {
			if (cancelled) return;
			// Mark read only now that it is on screen — and not at all if the reader has
			// already gone (e.g. "mark unread" pressed while this was loading).
			if (!d.flags.seen) {
				d.flags.seen = true;
				api('POST', '/messages/flag', {
					folder: key.folder,
					uids: [key.uid],
					flag: 'seen',
					value: true
				}).catch(() => {});
			}
			detail = d;
			onloaded?.(d);
		})
		.catch((e) => !cancelled && fail(e))
		.finally(() => !cancelled && (loading = false));
	return () => {
		cancelled = true;
	};
});

let added = $state(false);
async function addSender() {
	if (!detail?.from) return;
	try {
		await api('POST', '/contacts', {
			id: null,
			name: detail.from.name ?? '',
			email: detail.from.email
		});
		added = true;
		toast(`${display(detail.from)} added to contacts`);
	} catch (e) {
		fail(e);
	}
}

const attachmentUrl = (index: number, inline = false) =>
	`/api/attachment?${qs({ folder, uid, index, inline: inline ? 1 : undefined })}`;
const previewable = (t: string) =>
	['image/png', 'image/jpeg', 'image/gif', 'image/webp'].includes(t);
const imageAttachments = $derived(
	detail?.attachments.filter((a) => previewable(a.content_type)) ?? []
);
</script>

{#if detail && detail.uid === uid}
	<article aria-busy={loading}>
		<header>
			<h2>{detail.subject || '(no subject)'}</h2>
			<div class="meta">
				<div class="avatar" aria-hidden="true">
					{(display(detail.from) || '?').trim().charAt(0).toUpperCase()}
				</div>
				<div class="who">
					<div>
						<strong>{display(detail.from)}</strong>
						{#if detail.from?.name}<span class="addr">&lt;{detail.from.email}&gt;</span>{/if}
						{#if detail.from && !added}
							<button class="add-contact" title="Add to contacts" aria-label="Add sender to contacts" onclick={addSender}
								><Icon name="userPlus" size={14} /></button
							>
						{/if}
					</div>
					{#if detail.to.length}<div class="line">To: {fullList(detail.to)}</div>{/if}
					{#if detail.cc.length}<div class="line">Cc: {fullList(detail.cc)}</div>{/if}
					{#if detail.bcc.length}<div class="line">Bcc: {fullList(detail.bcc)}</div>{/if}
				</div>
				<time datetime={detail.date ?? undefined}>{longDate(detail.date)}</time>
			</div>
		</header>

		{#if detail.remote_images_blocked}
			<div class="notice">
				<Icon name="image" size={16} />
				<span>Remote images are hidden to protect your privacy.</span>
				<button class="link" onclick={() => (images = true)}>Show images</button>
			</div>
		{/if}

		{#if detail.attachments.length}
			<ul class="attachments" aria-label="Attachments">
				{#each detail.attachments as a (a.index)}
					<li>
						<a href={attachmentUrl(a.index)} download={a.filename}>
							<Icon name="clip" size={15} />
							<span class="fname">{a.filename}</span>
							<span class="fsize">{size(a.size)}</span>
							<Icon name="download" size={15} />
						</a>
					</li>
				{/each}
			</ul>
		{/if}

		{#if imageAttachments.length}
			<div class="previews" aria-label="Image attachments">
				{#each imageAttachments as a (a.index)}
					<a href={attachmentUrl(a.index, true)} target="_blank" rel="noopener" title={a.filename}>
						<img src={attachmentUrl(a.index, true)} alt={a.filename} loading="lazy" />
					</a>
				{/each}
			</div>
		{/if}

		{#if detail.html}
			<!-- No allow-scripts and no allow-same-origin: the message cannot run code or reach
			     this app, even if something slipped past the server-side sanitizer. -->
			<iframe
				title="Message body"
				sandbox="allow-popups allow-popups-to-escape-sandbox"
				srcdoc={detail.html}
				referrerpolicy="no-referrer"
			></iframe>
		{:else}
			<pre class="text">{detail.text ?? ''}</pre>
		{/if}
	</article>
{:else}
	<div class="placeholder">{loading ? 'Loading…' : ''}</div>
{/if}

<style>
	article {
		display: flex;
		flex-direction: column;
		min-height: 0;
		flex: 1;
	}
	header {
		padding: 18px 24px 12px;
	}
	h2 {
		margin: 0 0 14px;
		font-size: 20px;
		font-weight: 650;
		line-height: 1.3;
		overflow-wrap: anywhere;
	}
	.meta {
		display: flex;
		gap: 12px;
		align-items: flex-start;
	}
	.avatar {
		flex: none;
		width: 38px;
		height: 38px;
		display: grid;
		place-items: center;
		border-radius: 50%;
		background: var(--accent-soft);
		color: var(--accent);
		font-weight: 700;
	}
	.who {
		flex: 1;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.addr,
	.line {
		color: var(--text-soft);
		font-size: 13px;
	}
	.add-contact {
		vertical-align: middle;
		margin-left: 4px;
		padding: 2px;
		border: 0;
		background: none;
		color: var(--text-faint);
		border-radius: 4px;
	}
	.add-contact:hover {
		color: var(--accent);
		background: var(--bg-hover);
	}
	time {
		flex: none;
		color: var(--text-faint);
		font-size: 12.5px;
	}
	.notice {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 0 24px 10px;
		padding: 8px 12px;
		border-radius: var(--radius);
		background: var(--bg-soft);
		color: var(--text-soft);
		font-size: 13px;
	}
	.link {
		border: 0;
		background: none;
		padding: 0;
		color: var(--accent);
		font-weight: 600;
	}
	.attachments {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		list-style: none;
		margin: 0;
		padding: 0 24px 12px;
	}
	.attachments a {
		display: flex;
		align-items: center;
		gap: 8px;
		max-width: 280px;
		padding: 7px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text);
		text-decoration: none;
		font-size: 13px;
	}
	.attachments a:hover {
		background: var(--bg-hover);
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
	.previews {
		display: flex;
		gap: 8px;
		flex-wrap: wrap;
		padding: 0 24px 12px;
	}
	.previews a {
		display: block;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
		background: var(--bg-soft);
	}
	.previews img {
		display: block;
		height: 110px;
		max-width: 220px;
		object-fit: cover;
	}
	iframe {
		flex: 1;
		min-height: 300px;
		width: calc(100% - 32px);
		margin: 0 16px 16px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: #fff;
	}
	.text {
		flex: 1;
		margin: 0;
		padding: 4px 24px 24px;
		overflow: auto;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		font: inherit;
	}
	.placeholder {
		flex: 1;
		display: grid;
		place-items: center;
		color: var(--text-faint);
	}
</style>
