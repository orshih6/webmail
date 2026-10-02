<script lang="ts">
import { cubicOut } from 'svelte/easing';
import { prefersReducedMotion } from 'svelte/motion';
import { slide } from 'svelte/transition';
import { get, qs } from '$lib/api/client';
import type { Thread } from '$lib/api/types/Thread';
import { display, shortDate } from '$lib/format';
import { app } from '$lib/state.svelte';
import MessageView from './MessageView.svelte';

let { folder, uid }: { folder: string; uid: number } = $props();

let thread = $state<Thread | null>(null);
let open = $state<Set<string>>(new Set());

$effect(() => {
	const key = { folder, uid };
	let cancelled = false;
	thread = null;
	open = new Set();
	const abort = new AbortController();
	get<Thread>(`/thread?${qs(key)}`, abort.signal)
		.then((t) => !cancelled && (thread = t))
		.catch(() => {}); // the message itself still shows; the strip is a bonus
	return () => {
		cancelled = true;
		abort.abort();
	};
});

const id = (f: string, u: number) => `${f}/${u}`;
const isCurrent = (f: string, u: number) => f === folder && u === uid;
const folderName = (p: string) => app.folders.find((f) => f.path === p)?.name ?? p;

function toggle(k: string) {
	const next = new Set(open);
	if (next.has(k)) next.delete(k);
	else next.add(k);
	open = next;
}
</script>

{#if thread && thread.items.length > 1}
	<!-- It arrives after the message: unfold it into place instead of shoving the message down. -->
	<section
		class="conversation"
		aria-label="Conversation"
		in:slide={{ duration: prefersReducedMotion.current ? 0 : 260, easing: cubicOut }}
	>
		<h3>Conversation · {thread.items.length} messages</h3>
		<ol>
			{#each thread.items as h (id(h.folder, h.message.uid))}
				{@const k = id(h.folder, h.message.uid)}
				{@const current = isCurrent(h.folder, h.message.uid)}
				<li class:current class:unread={!h.message.flags.seen && !current}>
					<button
						class="item"
						disabled={current}
						aria-expanded={current ? undefined : open.has(k)}
						onclick={() => toggle(k)}
					>
						<span class="who">{h.message.from ? display(h.message.from) : '—'}</span>
						{#if h.folder !== folder}<span class="badge">{folderName(h.folder)}</span>{/if}
						{#if current}<span class="badge here">Open below</span>{/if}
						<span class="date">{shortDate(h.message.date)}</span>
					</button>
					{#if open.has(k) && !current}
						<div class="body">
							<MessageView folder={h.folder} uid={h.message.uid} />
						</div>
					{/if}
				</li>
			{/each}
		</ol>
	</section>
{/if}

<style>
	.conversation {
		flex: none;
		max-height: 45%;
		overflow-y: auto;
		margin: 0 16px;
		border-bottom: 1px solid var(--border);
	}
	h3 {
		margin: 12px 8px 6px;
		font-size: 12px;
		font-weight: 600;
		letter-spacing: 0.03em;
		text-transform: uppercase;
		color: var(--text-faint);
	}
	ol {
		list-style: none;
		margin: 0 0 8px;
		padding: 0;
	}
	li {
		border-radius: var(--radius);
	}
	.item {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 7px 8px;
		border: 0;
		border-radius: var(--radius);
		background: none;
		text-align: left;
		color: var(--text-soft);
	}
	.item:hover:not(:disabled) {
		background: var(--bg-hover);
	}
	.item:disabled {
		cursor: default;
	}
	.unread .who {
		font-weight: 650;
		color: var(--text);
	}
	.current .who {
		color: var(--text);
	}
	.who {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.badge {
		padding: 1px 7px;
		border-radius: 9px;
		background: var(--bg-active);
		font-size: 11px;
		font-weight: 600;
		white-space: nowrap;
	}
	.badge.here {
		background: var(--accent-soft);
		color: var(--accent);
	}
	.date {
		font-size: 12px;
		color: var(--text-faint);
		white-space: nowrap;
	}
	.body {
		display: flex;
		flex-direction: column;
		margin: 4px 0 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
	}
</style>
