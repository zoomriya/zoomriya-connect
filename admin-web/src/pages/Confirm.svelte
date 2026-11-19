<script lang="ts">
	import { ui } from '../lib/ui.svelte';

	let {
		text,
		action,
		danger = false,
		onconfirm,
		oncancel
	}: { text: string; action: string; danger?: boolean; onconfirm: () => void; oncancel: () => void } = $props();
	const w = $derived(ui.w);

	function key(e: KeyboardEvent) {
		if (e.key === 'Escape') oncancel();
	}
</script>

<svelte:window onkeydown={key} />

<div class="backdrop" role="presentation">
	<div class="card sheet" role="alertdialog" aria-modal="true" aria-label={action}>
		<p>{text}</p>
		<div class="actions">
			<button class="btn" onclick={oncancel}>{w.cancel}</button>
			<button class="btn {danger ? 'danger' : 'primary'}" onclick={onconfirm}>{action}</button>
		</div>
	</div>
</div>
