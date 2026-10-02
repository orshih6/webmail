import type { Folder } from '$lib/api/types/Folder';
import type { Prefs } from '$lib/api/types/Prefs';
import type { PublicConfig } from '$lib/api/types/PublicConfig';
import type { Special } from '$lib/api/types/Special';
import type { Theme } from '$lib/api/types/Theme';

/** This deployment's name, version and source link (from /api/config). */
export const brand = $state<PublicConfig>({
	app_name: 'Webmail',
	version: '',
	license: 'AGPL-3.0-only',
	source_url: 'https://github.com/orshih6/webmail'
});

/** Page title: "Inbox (3) · Webmail". */
export const pageTitle = (what?: string) => (what ? `${what} · ${brand.app_name}` : brand.app_name);

export const app = $state({
	email: '',
	csrf: '',
	folders: [] as Folder[],
	/** Bumped by live updates; pages watching it refetch. */
	changed: 0,
	/** Mobile: folder drawer open. */
	drawer: false,
	/** 'mail': the mail server is unreachable; 'offline': the browser is. Drives the banner. */
	connection: 'ok' as 'ok' | 'mail' | 'offline',
	/** Read out by screen readers (polite live region in the mail layout). */
	announcement: '',
	/** Until this time (ms), unread increases are the user's own doing (undo, move back,
	 *  mark unread) and must not be announced as new mail. */
	selfChangeUntil: 0,
	/** Set while the "session expired, sign in again" dialog is open. */
	reauth: null as null | { resolve: (ok: boolean) => void },
	/** Messages being dragged from the list onto a folder. */
	drag: null as { folder: string; uids: number[] } | null,
	prefs: {
		page_size: 50,
		conversations: true,
		theme: 'system',
		remote_images: 'never'
	} as Prefs
});

/** Applies a theme to <html> and remembers it so the next load (and the sign-in page) starts right. */
export function applyTheme(t: Theme) {
	if (t === 'system') document.documentElement.removeAttribute('data-theme');
	else document.documentElement.dataset.theme = t;
	try {
		localStorage.setItem('theme', t);
	} catch {}
}

export function specialPath(s: Special): string | undefined {
	return app.folders.find((f) => f.special === s)?.path;
}

type Toast = {
	id: number;
	text: string;
	kind: 'info' | 'error';
	action?: { label: string; run: () => void };
};

export const toasts = $state<Toast[]>([]);
let nextToast = 1;

export function toast(text: string, kind: Toast['kind'] = 'info', action?: Toast['action']) {
	const id = nextToast++;
	toasts.push({ id, text, kind, action });
	// Toasts with an action (Undo) stay long enough to reach for it.
	setTimeout(() => dismiss(id), action ? 8000 : kind === 'error' ? 7000 : 4000);
}

export function dismiss(id: number) {
	const i = toasts.findIndex((t) => t.id === id);
	if (i >= 0) toasts.splice(i, 1);
}

export function fail(e: unknown) {
	// The connection banner already says the server is unreachable; don't stack toasts on it.
	// A failed send is the exception: its message says whether to check Sent.
	if (
		e instanceof Error &&
		'transient' in e &&
		e.transient &&
		(e as { path?: string }).path !== '/send'
	)
		return;
	if (e instanceof Error && e.message) toast(e.message, 'error');
	else toast('Something went wrong', 'error');
}

/** A copy of the message being written, kept in this browser until the server has it (as
 *  a draft or a sent message). It only outlives the composer when saving failed — expired
 *  session, unreachable server, closed tab — and is offered back on the next compose. */
export type ComposeBackup = {
	savedAt: number;
	mode: string;
	source: { folder: string; uid: number } | null;
	draft: { folder: string; uid: number } | null;
	to: string;
	cc: string;
	bcc: string;
	subject: string;
	text: string;
	html: string;
	rich: boolean;
	inReplyTo: string | null;
	references: string[];
	identityId: string;
	hadAttachments: boolean;
};

const backupKey = () => `compose-backup:${app.email.toLowerCase()}`;

export function saveComposeBackup(b: ComposeBackup) {
	try {
		localStorage.setItem(backupKey(), JSON.stringify(b));
	} catch {}
}

export function loadComposeBackup(): ComposeBackup | null {
	try {
		const raw = localStorage.getItem(backupKey());
		return raw ? (JSON.parse(raw) as ComposeBackup) : null;
	} catch {
		return null;
	}
}

export function clearComposeBackup() {
	try {
		localStorage.removeItem(backupKey());
	} catch {}
}

/** On sign-out: unsent text must not linger in a shared browser. */
export function clearComposeBackups() {
	try {
		for (const k of Object.keys(localStorage))
			if (k.startsWith('compose-backup:')) localStorage.removeItem(k);
	} catch {}
}

/** "Moved to X · Undo" after a move/delete/junk. Undo moves the messages back. */
export function undoToast(
	text: string,
	from: string,
	result: { to: string | null; uids: number[] } | undefined,
	undo: (to: string, uids: number[], back: string) => Promise<void>
) {
	if (result?.to && result.uids.length) {
		const { to, uids } = result;
		toast(text, 'info', { label: 'Undo', run: () => void undo(to, uids, from) });
	} else {
		toast(text);
	}
}
