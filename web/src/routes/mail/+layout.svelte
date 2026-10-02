<script lang="ts">
import { goto } from '$app/navigation';
import { api } from '$lib/api/client';
import type { Folder } from '$lib/api/types/Folder';
import type { Prefs } from '$lib/api/types/Prefs';
import type { SessionInfo } from '$lib/api/types/SessionInfo';
import ConnectionBanner from '$lib/components/ConnectionBanner.svelte';
import ReauthDialog from '$lib/components/ReauthDialog.svelte';
import Sidebar from '$lib/components/Sidebar.svelte';
import Skeleton from '$lib/components/Skeleton.svelte';
import { app, applyTheme, fail } from '$lib/state.svelte';

let { children } = $props();
let ready = $state(false);

/** Lands on the list's current message (its one Tab stop), or the list itself if empty. */
function skipToMessages(e: MouseEvent) {
	const row = document.querySelector<HTMLElement>('.rows .row[tabindex="0"]');
	const target = row ?? document.getElementById('messages');
	if (!target) return;
	e.preventDefault();
	target.focus();
}

async function refreshFolders() {
	try {
		const before = app.folders.find((f) => f.special === 'inbox')?.unread;
		app.folders = await api<Folder[]>('GET', '/folders');
		const after = app.folders.find((f) => f.special === 'inbox')?.unread;
		// Only mail that arrived on its own is "new"; an Undo or "mark unread" also raises
		// the count but must not be announced as new mail.
		if (
			before !== undefined &&
			after !== undefined &&
			after > before &&
			Date.now() > app.selfChangeUntil
		) {
			const n = after - before;
			app.announcement = `${n} new message${n === 1 ? '' : 's'} in Inbox`;
		}
	} catch (e) {
		fail(e);
	}
}

// Session check, then folders, then live updates for as long as the shell is mounted.
$effect(() => {
	let events: EventSource | undefined;
	let timer: ReturnType<typeof setTimeout> | undefined;
	const onFocus = () => app.changed++;

	(async () => {
		if (!app.email) {
			try {
				const s = await api<SessionInfo>('GET', '/session');
				app.email = s.email;
				app.csrf = s.csrf;
			} catch {
				return; // api() already redirected to /login
			}
		}
		try {
			app.prefs = await api<Prefs>('GET', '/prefs');
			applyTheme(app.prefs.theme);
		} catch {}
		ready = true;
		await refreshFolders();
		events = new EventSource('/api/events');
		// EventSource reconnects by itself; anything that changed meanwhile was missed, so
		// refresh once the stream is back after an error.
		let broken = false;
		events.addEventListener('error', () => (broken = true));
		events.addEventListener('open', () => {
			if (broken) app.changed++;
			broken = false;
		});
		events.addEventListener('change', () => {
			// Several IMAP notifications arrive per new message; refetch once.
			clearTimeout(timer);
			timer = setTimeout(() => app.changed++, 400);
		});
		window.addEventListener('focus', onFocus);
	})();

	return () => {
		events?.close();
		clearTimeout(timer);
		window.removeEventListener('focus', onFocus);
	};
});

// Live changes and window focus also refresh the unread counts.
$effect(() => {
	if (app.changed > 0) refreshFolders();
});

$effect(() => {
	if (ready && !app.email) goto('/login');
});
</script>

{#if ready}
	<div class="shell">
		<a class="skip" href="#messages" onclick={skipToMessages}>Skip to messages</a>
		<div class="sr-only" aria-live="polite" aria-atomic="true">{app.announcement}</div>
		<ConnectionBanner />
		<ReauthDialog />
		<Sidebar onrefresh={() => app.changed++} />
		<div class="main">{@render children()}</div>
	</div>
{:else}
	<!-- Checking the session: show the shape of the app, not a blank page. -->
	<div class="shell boot" aria-busy="true">
		<span class="sr-only" role="status">Loading your mailbox…</span>
		<div class="boot-nav">
			<div class="boot-brand"><img src="/icons/logo.svg" alt="" width="26" height="26" /></div>
			<span class="sk boot-compose"></span>
			<Skeleton kind="folders" count={5} />
		</div>
		<div class="boot-list"><Skeleton kind="rows" count={9} /></div>
	</div>
{/if}

<style>
	.shell {
		display: flex;
		height: 100vh;
		height: 100dvh;
	}
	.skip {
		position: fixed;
		top: 8px;
		left: 8px;
		z-index: 100;
		padding: 8px 14px;
		border-radius: var(--radius);
		background: var(--accent);
		color: var(--accent-text);
		font-weight: 600;
		transform: translateY(-200%);
	}
	.skip:focus {
		transform: none;
	}
	.boot-nav {
		width: 240px;
		flex: none;
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 14px 10px;
		background: var(--bg-soft);
		border-right: 1px solid var(--border);
	}
	.boot-brand {
		padding: 2px 8px 6px;
	}
	.boot-compose {
		height: 40px;
		margin: 0 4px 10px;
		border-radius: var(--radius);
	}
	.boot-list {
		width: 420px;
		padding-top: 52px;
		border-right: 1px solid var(--border);
	}
	@media (max-width: 760px) {
		.boot-nav {
			display: none;
		}
		.boot-list {
			width: 100%;
			border-right: 0;
		}
	}
	.main {
		flex: 1;
		min-width: 0;
		display: flex;
	}
</style>
