<script lang="ts">
import { goto } from '$app/navigation';
import { api } from '$lib/api/client';
import type { SessionInfo } from '$lib/api/types/SessionInfo';
import Spinner from '$lib/components/Spinner.svelte';
import { app, brand, pageTitle } from '$lib/state.svelte';

let email = $state('');
let password = $state('');
let busy = $state(false);
let error = $state('');

async function submit(e: SubmitEvent) {
	e.preventDefault();
	busy = true;
	error = '';
	try {
		const s = await api<SessionInfo>('POST', '/login', { email, password });
		app.email = s.email;
		app.csrf = s.csrf;
		await goto('/mail?f=INBOX', { replaceState: true });
	} catch (err) {
		error = err instanceof Error ? err.message : 'Sign-in failed';
		password = '';
	} finally {
		busy = false;
	}
}
</script>

<svelte:head><title>{pageTitle('Sign in')}</title></svelte:head>

<main>
	<form onsubmit={submit}>
		<div class="brand">
			<img src="/icons/logo.svg" alt="" width="44" height="44" />
			<span>{brand.app_name}</span>
		</div>
		<h1>Sign in to your mailbox</h1>
		<label>
			<span>Email address</span>
			<!-- svelte-ignore a11y_autofocus -->
			<input class="field" type="email" autocomplete="username" required autofocus bind:value={email} />
		</label>
		<label>
			<span>Password</span>
			<input
				class="field"
				type="password"
				autocomplete="current-password"
				required
				bind:value={password}
			/>
		</label>
		{#if error}<p class="error" role="alert">{error}</p>{/if}
		<button class="btn primary" type="submit" disabled={busy}>
			{#if busy}<Spinner />{/if}
			{busy ? 'Signing in…' : 'Sign in'}
		</button>
	</form>
	<p class="foot">
		<a href={brand.source_url} rel="noopener" target="_blank">{brand.app_name}</a> is free software
		(AGPL-3.0){brand.version ? ` · v${brand.version}` : ''}
	</p>
</main>

<style>
	main {
		min-height: 100%;
		display: grid;
		place-items: center;
		padding: 24px;
		background: var(--bg-soft);
	}
	form {
		width: 100%;
		max-width: 360px;
		display: flex;
		flex-direction: column;
		gap: 16px;
		padding: 32px;
		border: 1px solid var(--border);
		border-radius: 14px;
		background: var(--bg);
		box-shadow: var(--shadow);
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 12px;
		font-size: 20px;
		font-weight: 700;
		letter-spacing: -0.01em;
	}
	h1 {
		margin: 0 0 4px;
		font-size: 20px;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 6px;
		font-size: 13px;
		color: var(--text-soft);
		font-weight: 500;
	}
	.btn {
		height: 40px;
		justify-content: center;
	}
	main {
		grid-template-rows: 1fr auto;
	}
	.foot {
		margin: 16px 0 0;
		font-size: 12.5px;
		color: var(--text-faint);
		text-align: center;
	}
	.foot a {
		color: inherit;
	}
	.error {
		margin: 0;
		color: var(--danger);
	}
</style>
