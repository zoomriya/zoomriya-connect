<script lang="ts">
	import { ui } from '../lib/ui.svelte';

	let { onsubmit }: { onsubmit: (token: string) => Promise<string | null> } = $props();
	let value = $state('');
	let problem = $state<string | null>(null);
	let busy = $state(false);
	const w = $derived(ui.w);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (!value.trim()) return;
		busy = true;
		problem = await onsubmit(value);
		busy = false;
	}
</script>

<div class="signin">
	<form class="card" onsubmit={submit}>
		<div class="brand flush">
			<div class="brand-mark" aria-hidden="true">Z</div>
			<div class="brand-name">{w.product}</div>
		</div>
		<h1>{w.signInTitle}</h1>
		<p class="secondary">{w.signInHelp}</p>
		<label>
			<span class="side-label">{w.token}</span>
			<input
				class="input mono"
				type="password"
				autocomplete="off"
				spellcheck="false"
				bind:value
			/>
		</label>
		{#if problem}<p class="notice bad" role="alert">{problem}</p>{/if}
		<button class="btn primary" type="submit" disabled={busy || !value.trim()}>{w.signIn}</button>
	</form>
</div>
