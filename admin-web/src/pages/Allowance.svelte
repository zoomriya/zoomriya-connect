<script lang="ts">
	import { untrack } from 'svelte';
	import { put, type Allowances, type ControlPlaneOverview } from '../lib/api';
	import { count, size } from '../lib/format';
	import { ui } from '../lib/ui.svelte';

	let { overview, onchange }: { overview: ControlPlaneOverview; onchange: () => void } = $props();
	const w = $derived(ui.w);
	// The form starts from what's set now; Mbit/s on screen, kB/s on the wire.
	const start = untrack(() => overview.allowances);
	let defaultGb = $state(start.default_gb);
	let factoryGb = $state(start.factory_gb);
	let mbits = $state(Math.round((start.slow_kbps * 8) / 100) / 10);
	let result = $state<{ ok: boolean; text: string } | null>(null);
	let busy = $state(false);

	const valid = $derived(
		Number.isInteger(defaultGb) && defaultGb >= 0 && Number.isInteger(factoryGb) && factoryGb >= 0 && mbits >= 0.1
	);

	async function save() {
		busy = true;
		const body: Allowances = {
			default_gb: defaultGb,
			factory_gb: factoryGb,
			slow_kbps: Math.max(1, Math.round((mbits * 1000) / 8))
		};
		try {
			await put('/allowances', body);
			result = { ok: true, text: w.saved };
			onchange();
		} catch (e) {
			result = { ok: false, text: String((e as Error).message) };
		}
		busy = false;
	}
</script>

<div class="page-head"><h1>{w.allowance}</h1></div>

<div class="stack">
	<div class="grid stats">
		<div class="card stat">
			<div class="stat-label">{w.relayUseThisMonth}</div>
			<div class="stat-value">{size(overview.relay_use.bytes, ui.lang)}</div>
		</div>
		<div class="card stat">
			<div class="stat-label">{w.turnUseThisMonth}</div>
			<div class="stat-value">{size(overview.relay_use.turn, ui.lang)}</div>
		</div>
		<div class="card stat">
			<div class="stat-label">{w.connectedBoxes}</div>
			<div class="stat-value">{count(overview.connected.boxes, ui.lang)}</div>
		</div>
	</div>
	<section class="card stack">
		<p class="secondary">{w.allowanceHelp}</p>
		<form class="stack" onsubmit={(e) => { e.preventDefault(); if (valid) save(); }}>
			<label class="field">
				<span>{w.allowanceSelfInstalled}</span>
				<span class="with-unit"><input class="input" type="number" min="0" step="1" bind:value={defaultGb} /><span class="muted">{w.gbAMonth}</span></span>
			</label>
			<label class="field">
				<span>{w.allowanceFactory}</span>
				<span class="with-unit"><input class="input" type="number" min="0" step="1" bind:value={factoryGb} /><span class="muted">{w.gbAMonth}</span></span>
			</label>
			<label class="field">
				<span>{w.slowSpeed}</span>
				<span class="with-unit"><input class="input" type="number" min="0.1" step="0.1" bind:value={mbits} /><span class="muted">{w.mbits}</span></span>
			</label>
			{#if result}<p class="notice {result.ok ? 'good' : 'bad'}">{result.text}</p>{/if}
			<div class="actions"><button class="btn primary" type="submit" disabled={busy || !valid}>{w.save}</button></div>
		</form>
	</section>
</div>
