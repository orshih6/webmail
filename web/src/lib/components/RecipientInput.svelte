<script lang="ts">
import { api, qs } from '$lib/api/client';
import type { Contact } from '$lib/api/types/Contact';
import { full } from '$lib/format';

let {
	value = $bindable(''),
	input = $bindable<HTMLInputElement>(),
	label
}: { value: string; input?: HTMLInputElement; label: string } = $props();

let hits = $state<Contact[]>([]);
let active = $state(0);
let timer: ReturnType<typeof setTimeout> | undefined;
let seq = 0;
const listId = `rcpt-${Math.random().toString(36).slice(2)}`;

/** The entry being typed: everything after the last comma. */
const token = () => value.slice(value.lastIndexOf(',') + 1).trim();

function oninput() {
	clearTimeout(timer);
	const q = token();
	if (q.length < 2) {
		hits = [];
		return;
	}
	const mine = ++seq;
	timer = setTimeout(async () => {
		try {
			const res = await api<Contact[]>('GET', `/contacts?${qs({ q })}`);
			if (mine !== seq) return; // a newer keystroke won
			const already = value.toLowerCase();
			hits = res.filter((c) => !already.includes(`<${c.email.toLowerCase()}>`));
			active = 0;
		} catch {
			hits = [];
		}
	}, 120);
}

function pick(c: Contact) {
	const head = value.slice(0, value.lastIndexOf(',') + 1);
	value = `${head ? `${head} ` : ''}${full({ name: c.name || null, email: c.email })}, `;
	hits = [];
	input?.focus();
}

function onkeydown(e: KeyboardEvent) {
	if (!hits.length) return;
	if (e.key === 'ArrowDown') active = (active + 1) % hits.length;
	else if (e.key === 'ArrowUp') active = (active - 1 + hits.length) % hits.length;
	else if (e.key === 'Enter' || e.key === 'Tab') pick(hits[active]);
	else if (e.key === 'Escape') hits = [];
	else return;
	e.preventDefault();
	e.stopPropagation();
}
</script>

<div class="wrap">
	<input
		bind:this={input}
		bind:value
		{oninput}
		{onkeydown}
		onblur={() => setTimeout(() => (hits = []), 150)}
		autocomplete="off"
		spellcheck="false"
		aria-label={label}
		role="combobox"
		aria-expanded={hits.length > 0}
		aria-controls={listId}
		aria-autocomplete="list"
		aria-activedescendant={hits.length ? `${listId}-${active}` : undefined}
	/>
	{#if hits.length}
		<ul id={listId} role="listbox">
			{#each hits as c, i (c.id)}
				<li
					id="{listId}-{i}"
					role="option"
					aria-selected={i === active}
					class:active={i === active}
					onmousedown={(e) => {
						e.preventDefault();
						pick(c);
					}}
				>
					{#if c.name}<strong>{c.name}</strong>{/if}
					<span>{c.email}</span>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.wrap {
		position: relative;
		flex: 1;
		min-width: 0;
	}
	input {
		width: 100%;
		height: 42px;
		border: 0;
		outline: none;
		background: none;
	}
	ul {
		position: absolute;
		top: 100%;
		left: -8px;
		z-index: 20;
		min-width: 300px;
		max-width: min(480px, 90vw);
		margin: 0;
		padding: 4px;
		list-style: none;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg);
		box-shadow: var(--shadow);
	}
	li {
		display: flex;
		gap: 8px;
		align-items: baseline;
		padding: 7px 10px;
		border-radius: 6px;
		cursor: pointer;
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
	}
	li.active {
		background: var(--accent-soft);
	}
	li span {
		color: var(--text-soft);
		font-size: 13px;
	}
</style>
