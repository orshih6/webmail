<script lang="ts">
import { goto } from '$app/navigation';

$effect(() => {
	goto('/mail?f=INBOX', { replaceState: true });
});
</script>
