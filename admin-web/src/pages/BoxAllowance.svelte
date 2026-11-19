<script lang="ts">
	import { untrack } from 'svelte';
	import type { BoxSummary } from '../lib/api';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';

	let {
		box,
		usual,
		onsave,
		oncancel
	}: { box: BoxSummary; usual: number; onsave: (gb: number | null) => void; oncancel: () => void } = $props();
	const w = $derived(ui.w);
	let gb = $state(untrack(() => box.own_allowance_gb ?? box.allowance_gb));
	const usualText = $derived(usual === 0 ? w.noLimit : `${usual} ${w.gbAMonth}`);

	function key(e: KeyboardEvent) {
		if (e.key === 'Escape') oncancel();
	}
</script>

<svelte:window onkeydown={key} />

<div class="backdrop" role="presentation">
	<div class="card sheet" role="dialog" aria-modal="true" aria-label={fill(w.boxAllowanceTitle, { name: box.name })}>
	<form
		class="stack"
		onsubmit={(e) => { e.preventDefault(); if (Number.isInteger(gb) && gb >= 0) onsave(gb); }}
	>
		<h2 class="flush">{fill(w.boxAllowanceTitle, { name: box.name })}</h2>
		<p class="secondary">{w.boxAllowanceHelp}</p>
		<label class="field">
			<span>{w.ownAllowance}</span>
			<span class="with-unit"><input class="input" type="number" min="0" step="1" bind:value={gb} /><span class="muted">{w.gbAMonth}</span></span>
		</label>
		<p class="muted">{fill(w.usualIs, { v: usualText })}</p>
		<div class="actions">
			<button class="btn" type="button" onclick={oncancel}>{w.cancel}</button>
			{#if box.own_allowance_gb !== null}
				<button class="btn" type="button" onclick={() => onsave(null)}>{w.useUsual}</button>
			{/if}
			<button class="btn primary" type="submit" disabled={!Number.isInteger(gb) || gb < 0}>{w.save}</button>
		</div>
	</form>
	</div>
</div>
