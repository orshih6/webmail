import type { Address } from '$lib/api/types/Address';

export const display = (a: Address | null | undefined) => (a ? a.name || a.email : '');

export const full = (a: Address) => (a.name ? `${quoteName(a.name)} <${a.email}>` : a.email);

export const fullList = (list: Address[]) => list.map(full).join(', ');

function quoteName(n: string) {
	return /[,;<>@"()]/.test(n) ? `"${n.replace(/["\\]/g, '\\$&')}"` : n;
}

/** Today: 14:05 · this year: 3 Oct · older: 3 Oct 2024 */
export function shortDate(iso: string | null): string {
	if (!iso) return '';
	const d = new Date(iso);
	const now = new Date();
	if (d.toDateString() === now.toDateString())
		return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
	if (d.getFullYear() === now.getFullYear())
		return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
	return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}

export const longDate = (iso: string | null) =>
	iso ? new Date(iso).toLocaleString(undefined, { dateStyle: 'full', timeStyle: 'short' }) : '';

export function size(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function folderUrl(path: string, extra: Record<string, string | number> = {}) {
	const p = new URLSearchParams({ f: path });
	for (const [k, v] of Object.entries(extra)) p.set(k, String(v));
	return `/mail?${p}`;
}
