<script lang="ts">
import { api } from '$lib/api/client';
import type { SessionInfo } from '$lib/api/types/SessionInfo';
import { app, clearComposeBackups } from '$lib/state.svelte';

let password = $state('');
let busy = $state(false);
let error = $state('');
let dialog = $state<HTMLDialogElement>();

$effect(() => {
	if (app.reauth && dialog && !dialog.open) {
		password = '';
		error = '';
		dialog.showModal();
	}
	if (!app.reauth && dialog?.open) dialog.close();
});

async function submit(e: SubmitEvent) {
	e.preventDefault();
	busy = true;
	error = '';
	try {
		const s = await api<SessionInfo>('POST', '/login', { email: app.email, password });
		app.csrf = s.csrf;
		app.reauth?.resolve(true);
	} catch (err) {
		error = err instanceof Error ? err.message : 'Sign-in failed';
		password = '';
	} finally {
		busy = false;
	}
}

function leave() {
	clearComposeBackups();
	app.reauth?.resolve(false);
}
</script>

<dialog bind:this={dialog} oncancel={(e) => e.preventDefault()} aria-labelledby="reauth-title">
	<form onsubmit={submit}>
		<h2 id="reauth-title">Your session expired</h2>
		<p>Enter your password to carry on. Nothing you were doing has been lost.</p>
		<div class="who">{app.email}</div>
		<!-- svelte-ignore a11y_autofocus -->
		<input
			class="field"
			type="password"
			autocomplete="current-password"
			aria-label="Password"
			placeholder="Password"
			required
			autofocus
			bind:value={password}
		/>
		{#if error}<p class="error" role="alert">{error}</p>{/if}
		<div class="actions">
			<button class="btn primary" type="submit" disabled={busy}>{busy ? 'Signing in…' : 'Continue'}</button>
			<button class="btn" type="button" onclick={leave}>Use a different account</button>
		</div>
	</form>
</dialog>

<style>
	dialog {
		width: min(380px, calc(100vw - 32px));
		padding: 24px;
		border: 1px solid var(--border);
		border-radius: 14px;
		background: var(--bg);
		color: var(--text);
		box-shadow: var(--shadow);
	}
	dialog::backdrop {
		background: rgb(0 0 0 / 0.4);
	}
	form {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	h2 {
		margin: 0;
		font-size: 18px;
	}
	p {
		margin: 0;
		color: var(--text-soft);
	}
	.who {
		font-weight: 600;
	}
	.error {
		color: var(--danger);
	}
	.actions {
		display: flex;
		gap: 8px;
		flex-wrap: wrap;
	}
</style>
