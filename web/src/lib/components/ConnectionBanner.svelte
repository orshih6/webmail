<script lang="ts">
import { api } from '$lib/api/client';
import { app } from '$lib/state.svelte';

/** Seconds until the next automatic attempt (shown), and whether one is running. */
let wait = $state(0);
let trying = $state(false);
let delay = 2;
let timer: ReturnType<typeof setInterval> | undefined;

async function retry() {
	if (trying) return;
	trying = true;
	try {
		await api('GET', '/folders'); // success flips app.connection back to 'ok'
	} catch {
		delay = Math.min(delay * 2, 30);
	} finally {
		trying = false;
		wait = delay;
	}
}

// While the mail server is unreachable: count down, retry, back off (2s → 30s).
$effect(() => {
	if (app.connection !== 'mail') return;
	delay = 2;
	wait = delay;
	timer = setInterval(() => {
		if (trying) return;
		if (--wait <= 0) retry();
	}, 1000);
	return () => {
		clearInterval(timer);
		// Back to normal: refresh whatever the user is looking at.
		if (app.connection === 'ok') app.changed++;
	};
});

// The browser's own connectivity.
$effect(() => {
	const off = () => (app.connection = 'offline');
	const on = () => {
		app.connection = 'ok';
		app.changed++;
	};
	if (!navigator.onLine) off();
	window.addEventListener('offline', off);
	window.addEventListener('online', on);
	return () => {
		window.removeEventListener('offline', off);
		window.removeEventListener('online', on);
	};
});
</script>

{#if app.connection !== 'ok'}
	<div class="banner" role="status" aria-live="polite">
		<span class="dot" aria-hidden="true"></span>
		{#if app.connection === 'offline'}
			<span>You're offline. Mail will refresh when you're back.</span>
		{:else}
			<span>Can't reach the mail server. {trying ? 'Retrying…' : `Retrying in ${wait}s.`}</span>
			<button onclick={retry} disabled={trying}>Retry now</button>
		{/if}
	</div>
{/if}

<style>
	.banner {
		position: fixed;
		top: 10px;
		left: 50%;
		transform: translateX(-50%);
		z-index: 90;
		display: flex;
		align-items: center;
		gap: 10px;
		max-width: calc(100vw - 24px);
		padding: 8px 10px 8px 14px;
		border: 1px solid var(--border);
		border-radius: 20px;
		background: var(--bg);
		box-shadow: var(--shadow);
		font-size: 13px;
	}
	.dot {
		width: 8px;
		height: 8px;
		flex: none;
		border-radius: 50%;
		background: var(--star);
		animation: pulse 1.4s ease-in-out infinite;
	}
	button {
		height: 26px;
		padding: 0 10px;
		border: 1px solid var(--border);
		border-radius: 13px;
		background: var(--bg-soft);
		font-size: 12.5px;
		font-weight: 600;
	}
	@keyframes pulse {
		50% {
			opacity: 0.35;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.dot {
			animation: none;
		}
	}
</style>
