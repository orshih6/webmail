<script lang="ts">
import { dismiss, toasts } from '$lib/state.svelte';
</script>

<div class="toasts" role="status" aria-live="polite">
	{#each toasts as t (t.id)}
		<div class="toast" class:error={t.kind === 'error'}>
			<span>{t.text}</span>
			{#if t.action}
				<button
					class="action"
					onclick={() => {
						t.action?.run();
						dismiss(t.id);
					}}>{t.action.label}</button
				>
			{/if}
			<button class="close" aria-label="Dismiss" onclick={() => dismiss(t.id)}>×</button>
		</div>
	{/each}
</div>

<style>
	.toasts {
		position: fixed;
		bottom: 20px;
		left: 50%;
		transform: translateX(-50%);
		display: flex;
		flex-direction: column;
		gap: 8px;
		z-index: 100;
		width: max-content;
		max-width: calc(100vw - 32px);
	}
	.toast {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 10px 10px 10px 16px;
		border-radius: var(--radius);
		background: var(--text);
		color: var(--bg);
		box-shadow: var(--shadow);
		animation: rise 0.18s ease-out;
	}
	.toast.error {
		background: var(--danger);
		color: #fff;
	}
	.action {
		border: 0;
		background: none;
		color: inherit;
		font-weight: 700;
		text-decoration: underline;
	}
	.close {
		border: 0;
		background: none;
		color: inherit;
		opacity: 0.7;
		font-size: 18px;
		line-height: 1;
	}
	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(8px);
		}
	}
</style>
