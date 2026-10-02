<script lang="ts">
import type { Snippet } from 'svelte';

export type MenuItem = { label: string; run: () => void; danger?: boolean } | 'separator';

let {
	items,
	label,
	trigger,
	align = 'right'
}: { items: MenuItem[]; label: string; trigger: Snippet; align?: 'left' | 'right' } = $props();

let open = $state(false);
let root = $state<HTMLElement>();

function onwindowclick(e: MouseEvent) {
	if (open && root && !root.contains(e.target as Node)) open = false;
}

function onkeydown(e: KeyboardEvent) {
	if (!open) return;
	const buttons = [...(root?.querySelectorAll<HTMLButtonElement>('[role=menuitem]') ?? [])];
	const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
	if (e.key === 'Escape') {
		open = false;
		(root?.querySelector('.trigger') as HTMLElement | null)?.focus();
	} else if (e.key === 'ArrowDown') buttons[(i + 1) % buttons.length]?.focus();
	else if (e.key === 'ArrowUp') buttons[(i - 1 + buttons.length) % buttons.length]?.focus();
	else return;
	e.preventDefault();
	e.stopPropagation();
}
</script>

<svelte:window onclick={onwindowclick} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="menu" bind:this={root} {onkeydown}>
	<button
		class="icon-btn trigger"
		aria-label={label}
		title={label}
		aria-haspopup="menu"
		aria-expanded={open}
		onclick={(e) => {
			e.stopPropagation();
			open = !open;
			if (open) queueMicrotask(() => root?.querySelector<HTMLElement>('[role=menuitem]')?.focus());
		}}>{@render trigger()}</button
	>
	{#if open}
		<div class="pop" class:left={align === 'left'} role="menu">
			{#each items as item, i (i)}
				{#if item === 'separator'}
					<hr />
				{:else}
					<button
						role="menuitem"
						class:danger={item.danger}
						onclick={(e) => {
							e.stopPropagation();
							open = false;
							item.run();
						}}>{item.label}</button
					>
				{/if}
			{/each}
		</div>
	{/if}
</div>

<style>
	.menu {
		position: relative;
		display: inline-flex;
	}
	.pop {
		position: absolute;
		top: calc(100% + 4px);
		right: 0;
		z-index: 30;
		min-width: 180px;
		padding: 4px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg);
		box-shadow: var(--shadow);
	}
	.pop.left {
		right: auto;
		left: 0;
	}
	[role='menuitem'] {
		display: block;
		width: 100%;
		padding: 7px 10px;
		border: 0;
		border-radius: 6px;
		background: none;
		text-align: left;
		white-space: nowrap;
	}
	[role='menuitem']:hover,
	[role='menuitem']:focus-visible {
		background: var(--bg-hover);
		outline: none;
	}
	.danger {
		color: var(--danger);
	}
	hr {
		margin: 4px 2px;
		border: 0;
		border-top: 1px solid var(--border);
	}
</style>
