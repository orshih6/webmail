<script lang="ts">
/** Placeholders shaped like the content they stand in for. Hidden from assistive tech;
 *  the surrounding container carries aria-busy and a spoken status. */
let {
	kind,
	count = 8
}: { kind: 'rows' | 'message' | 'folders' | 'lines' | 'form'; count?: number } = $props();

// Varied, stable widths so a column of placeholders reads like real text, not a grid.
const w = (i: number, min: number, span: number) => `${min + ((i * 37) % span)}%`;
</script>

<div class="skeleton {kind}" aria-hidden="true">
	{#if kind === 'rows'}
		{#each { length: count } as _, i (i)}
			<div class="row">
				<span class="sk who" style:width={w(i, 28, 30)}></span>
				<span class="sk date"></span>
				<span class="sk subject" style:width={w(i + 3, 45, 40)}></span>
			</div>
		{/each}
	{:else if kind === 'message'}
		<span class="sk title"></span>
		<div class="meta">
			<span class="sk avatar"></span>
			<div class="who">
				<span class="sk" style:width="34%"></span>
				<span class="sk" style:width="52%"></span>
			</div>
		</div>
		<div class="body">
			{#each { length: 7 } as _, i (i)}
				<span class="sk" style:width={i === 6 ? '38%' : w(i, 78, 22)}></span>
			{/each}
		</div>
	{:else if kind === 'folders'}
		{#each { length: count } as _, i (i)}
			<div class="folder">
				<span class="sk icon"></span>
				<span class="sk" style:width={w(i, 35, 35)}></span>
			</div>
		{/each}
	{:else if kind === 'form'}
		{#each { length: count } as _, i (i)}
			<div class="fieldrow">
				<span class="sk label"></span>
				<span class="sk" style:width={w(i, 40, 35)}></span>
			</div>
		{/each}
	{:else}
		{#each { length: count } as _, i (i)}
			<div class="line">
				<span class="sk circle"></span>
				<div class="stack">
					<span class="sk" style:width={w(i, 30, 30)}></span>
					<span class="sk" style:width={w(i + 2, 20, 25)}></span>
				</div>
			</div>
		{/each}
	{/if}
</div>

<style>
	.sk {
		height: 11px;
	}
	/* Message rows: same paddings and line rhythm as the real list. */
	.row {
		display: grid;
		grid-template-columns: 1fr auto;
		row-gap: 9px;
		padding: 14px 16px 15px 40px;
		border-bottom: 1px solid var(--border);
	}
	.subject {
		grid-column: 1 / -1;
	}
	.date {
		width: 42px;
		height: 9px;
	}
	/* Reader. */
	.message {
		padding: 22px 24px;
	}
	.title {
		width: 56%;
		height: 20px;
		margin-bottom: 20px;
		border-radius: 7px;
	}
	.meta {
		display: flex;
		gap: 12px;
		margin-bottom: 30px;
	}
	.avatar {
		flex: none;
		width: 38px;
		height: 38px;
		border-radius: 50%;
	}
	.who,
	.stack {
		flex: 1;
		display: flex;
		flex-direction: column;
		justify-content: center;
		gap: 8px;
	}
	.body {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	/* Sidebar folders. */
	.folder {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 34px;
		padding: 0 10px;
	}
	.icon {
		width: 17px;
		height: 17px;
		border-radius: 5px;
	}
	/* Settings / composer fields. */
	.fieldrow {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 44px;
		padding: 0 18px;
		border-bottom: 1px solid var(--border);
	}
	.label {
		width: 46px;
		height: 9px;
	}
	.line {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 12px 0;
		border-bottom: 1px solid var(--border);
	}
	.circle {
		flex: none;
		width: 34px;
		height: 34px;
		border-radius: 50%;
	}
</style>
