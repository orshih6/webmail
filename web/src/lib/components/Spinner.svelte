<script lang="ts">
/** Inline activity indicator for a pending action (Send, Sign in). Decorative: the
 *  button's text says what is happening. */
let { size = 14 }: { size?: number } = $props();
</script>

<svg class="spinner" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
	<circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="3" opacity="0.25" />
	<path d="M21 12a9 9 0 0 0-9-9" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
</svg>

<style>
	.spinner {
		flex: none;
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
</style>
