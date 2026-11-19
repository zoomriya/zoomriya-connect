<script lang="ts">
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';

	let { username, password, onclose }: { username: string; password: string; onclose: () => void } = $props();
	let copied = $state(false);
	const w = $derived(ui.w);

	async function copy() {
		try {
			await navigator.clipboard.writeText(password);
			copied = true;
		} catch {
			/* no clipboard: the password is on screen to copy by hand */
		}
	}
</script>

<div class="backdrop" role="presentation">
	<div class="card sheet" role="dialog" aria-modal="true" aria-label={fill(w.passwordFor, { name: username })}>
		<h2 class="flush">{fill(w.passwordFor, { name: username })}</h2>
		<div class="code selectable">{password}</div>
		<p class="secondary">{w.passwordOnce}</p>
		<div class="actions">
			<button class="btn" onclick={copy}>{copied ? w.copied : w.copy}</button>
			<button class="btn primary" onclick={onclose}>{w.close}</button>
		</div>
	</div>
</div>
