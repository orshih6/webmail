<script lang="ts">
import '../app.css';
import type { PublicConfig } from '$lib/api/types/PublicConfig';
import type { Theme } from '$lib/api/types/Theme';
import Toasts from '$lib/components/Toasts.svelte';
import { applyTheme, brand } from '$lib/state.svelte';

let { children } = $props();

try {
	applyTheme((localStorage.getItem('theme') as Theme | null) ?? 'system');
} catch {}

// Name, version and source link of this deployment; public, so fetched before sign-in.
$effect(() => {
	fetch('/api/config')
		.then((r) => (r.ok ? r.json() : null))
		.then((c: PublicConfig | null) => c && Object.assign(brand, c))
		.catch(() => {});
});
</script>

{@render children()}
<Toasts />
