<script lang="ts">
	import { post, type RelayOverview } from '../lib/api';
	import { count, shortKey } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';

	let { overview, onrefresh }: { overview: RelayOverview; onrefresh: () => void } = $props();
	let code = $state<{ code: string; minutes: number } | null>(null);
	let problem = $state<string | null>(null);
	const w = $derived(ui.w);
	const n = (v: number) => count(v, ui.lang);

	async function makeCode() {
		try {
			code = await post<{ code: string; minutes: number }>('/enrol-code');
			problem = null;
		} catch (e) {
			problem = String((e as Error).message);
		}
	}
</script>

<div class="page-head">
	<div>
		<h1>{w.relay}</h1>
		<p class="muted">
			{w.relayMode} {overview.mode === 'enrolled' ? w.modeEnrolled : w.modeControlPlane} · {fill(w.version, { v: overview.version })}
		</p>
	</div>
	<button class="btn" onclick={onrefresh}>{w.refresh}</button>
</div>

<div class="stack">
	<div class="grid stats">
		<div class="card stat"><div class="stat-label">{w.knownBoxes}</div><div class="stat-value">{n(overview.boxes)}</div></div>
		<div class="card stat"><div class="stat-label">{w.connectedBoxes}</div><div class="stat-value">{n(overview.connected.boxes)}</div></div>
		<div class="card stat"><div class="stat-label">{w.connectedDevices}</div><div class="stat-value">{n(overview.connected.devices)}</div></div>
		{#if overview.mode === 'control_plane'}
			<div class="card stat" class:warn={overview.slowed > 0}><div class="stat-label">{w.boxesSlowed}</div><div class="stat-value">{n(overview.slowed)}</div></div>
		{/if}
	</div>

	<section class="card">
		<h2>{w.connected}</h2>
		{#if overview.connected_boxes.length === 0}
			<p class="muted">{w.noneConnected}</p>
		{:else}
			<table class="table">
				<thead><tr><th>{w.key}</th><th>{w.devices}</th></tr></thead>
				<tbody>
					{#each overview.connected_boxes as b (b.key)}
						<tr><td class="mono" title={b.key}>{shortKey(b.key)}</td><td>{n(b.devices)}</td></tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</section>

	<div class="grid two">
		{#if overview.mode === 'enrolled'}
			<section class="card stack">
				<h2 class="flush">{w.addBox}</h2>
				<p class="secondary">{w.addBoxHelp}</p>
				{#if code}
					<div class="code" aria-live="polite">{code.code}</div>
					<p class="muted">{fill(w.codeWorks, { n: code.minutes })}</p>
				{/if}
				{#if problem}<p class="notice bad">{problem}</p>{/if}
				<div class="actions"><button class="btn primary" onclick={makeCode}>{w.makeCode}</button></div>
			</section>
		{/if}
		<section class="card">
			<h2>{w.sinceStart}</h2>
			<div class="rows">
				<div class="row"><span class="secondary">{w.allowedBoxes}</span><strong>{n(overview.since_start.boxes_allowed)}</strong></div>
				<div class="row"><span class="secondary">{w.allowedDevices}</span><strong>{n(overview.since_start.devices_allowed)}</strong></div>
				<div class="row"><span class="secondary">{w.refused}</span><strong>{n(overview.since_start.refused)}</strong></div>
				<div class="row"><span class="secondary">{w.refusedRemoved}</span><strong>{n(overview.since_start.refused_removed)}</strong></div>
				<div class="row"><span class="secondary">{w.refusedExpired}</span><strong>{n(overview.since_start.refused_expired)}</strong></div>
				<div class="row"><span class="secondary">{w.dropped}</span><strong>{n(overview.since_start.dropped)}</strong></div>
			</div>
		</section>
	</div>
</div>
