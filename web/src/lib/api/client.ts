import { goto } from '$app/navigation';
import { app } from '$lib/state.svelte';

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string,
		/** The mail server (or the network) was unreachable: the connection banner covers it. */
		public transient = false,
		public path = ''
	) {
		super(message);
	}
}

type Method = 'GET' | 'POST' | 'DELETE';

let reauthPending: Promise<boolean> | null = null;

/** Opens the sign-in dialog (once, however many requests hit 401 together) and resolves
 *  true when the user has signed in again, false if they chose to leave. */
function reauthenticate(): Promise<boolean> {
	reauthPending ??= new Promise<boolean>((resolve) => {
		app.reauth = {
			resolve: (ok) => {
				app.reauth = null;
				reauthPending = null;
				resolve(ok);
			}
		};
	});
	return reauthPending;
}

/** Endpoints that only touch our own database: their success says nothing about the mail
 *  server, so it must not clear the "can't reach the mail server" banner. */
const DB_ONLY = /^\/(prefs|identities|contacts|session|config|health|uploads\/)/;

/** Calls the Rust API. Adds the CSRF header to mutations; a 401 sends the user to sign-in;
 *  unreachable mail server / network flips `app.connection` for the banner. */
export async function api<T = void>(
	method: Method,
	path: string,
	body?: unknown,
	retried = false
): Promise<T> {
	const headers: Record<string, string> = {};
	const init: RequestInit = { method, headers, credentials: 'same-origin' };
	if (body instanceof FormData) {
		init.body = body;
	} else if (body !== undefined) {
		headers['content-type'] = 'application/json';
		init.body = JSON.stringify(body);
	}
	if (method !== 'GET') headers['x-csrf-token'] = app.csrf;

	let res: Response;
	try {
		res = await fetch(`/api${path}`, init);
	} catch {
		app.connection = navigator.onLine ? 'mail' : 'offline';
		throw new ApiError(0, 'Network error — check your connection', true, path);
	}
	if (res.status === 401 && path !== '/login') {
		// Mid-session expiry: sign in again over the page and retry, so nothing is lost.
		if (app.email && path !== '/session' && path !== '/logout' && !retried) {
			if (await reauthenticate()) return api<T>(method, path, body, true);
		}
		app.email = '';
		await goto('/login');
		throw new ApiError(401, 'Your session ended. Please sign in again.', false, path);
	}
	if (!res.ok) {
		let message = res.statusText || `Error ${res.status}`;
		let code: string | undefined;
		try {
			const b = await res.json();
			message = b.error ?? message;
			code = b.code;
		} catch {}
		// 502/504 come from the gateway when our server itself is restarting.
		const transient = code === 'mail_unavailable' || res.status === 502 || res.status === 504;
		if (transient) app.connection = 'mail';
		throw new ApiError(res.status, message, transient, path);
	}
	if (!DB_ONLY.test(path)) app.connection = 'ok';
	if (res.status === 204) return undefined as T;
	const text = await res.text();
	return (text ? JSON.parse(text) : undefined) as T;
}

export const qs = (params: Record<string, string | number | undefined | null>) =>
	new URLSearchParams(
		Object.entries(params)
			.filter(([, v]) => v !== undefined && v !== null && v !== '')
			.map(([k, v]) => [k, String(v)])
	).toString();
