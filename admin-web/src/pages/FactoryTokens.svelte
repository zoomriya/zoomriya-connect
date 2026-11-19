<script lang="ts">
	import { post, type ControlPlaneOverview } from '../lib/api';
	import { count } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';

	let { overview, onchange }: { overview: ControlPlaneOverview; onchange: () => void } = $props();
	let text = $state('');
	let result = $state<{ ok: boolean; text: string } | null>(null);
	let busy = $state(false);
	const w = $derived(ui.w);

	async function add() {
		busy = true;
		try {
			const r = await post<{ added: number }>('/factory-tokens', { tokens: text });
			result = { ok: true, text: fill(w.added, { n: r.added }) };
			text = '';
			onchange();
		} catch (e) {
			result = { ok: false, text: String((e as Error).message) };
		}
		busy = false;
	}
</script>

<div class="page-head"><h1>{w.factoryTokens}</h1></div>

<div class="stack">
	<div class="grid stats">
		<div class="card stat"><div class="stat-label">{w.unused}</div><div class="stat-value">{count(overview.factory_tokens.unused, ui.lang)}</div></div>
		<div class="card stat"><div class="stat-label">{w.used}</div><div class="stat-value">{count(overview.factory_tokens.used, ui.lang)}</div></div>
	</div>
	<section class="card stack">
		<p class="secondary">{w.tokensHelp}</p>
		<textarea class="textarea" spellcheck="false" bind:value={text}></textarea>
		{#if result}<p class="notice {result.ok ? 'good' : 'bad'}">{result.text}</p>{/if}
		<div class="actions"><button class="btn primary" disabled={busy || !text.trim()} onclick={add}>{w.addTokens}</button></div>
	</section>
</div>
