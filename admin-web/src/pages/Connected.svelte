<script lang="ts">
	import { onMount } from 'svelte';
	import { get, type ConnectedBox } from '../lib/api';
	import { count, shortKey } from '../lib/format';
	import { ui } from '../lib/ui.svelte';

	let list = $state<ConnectedBox[] | null>(null);
	let problem = $state<string | null>(null);
	const w = $derived(ui.w);

	async function load() {
		try {
			list = (await get<{ boxes: ConnectedBox[] }>('/connected')).boxes;
			problem = null;
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	onMount(() => {
		load();
		const tick = setInterval(load, 15_000);
		return () => clearInterval(tick);
	});
</script>

<div class="page-head">
	<h1>{w.connected}</h1>
	<button class="btn" onclick={load}>{w.refresh}</button>
</div>

{#if problem}<p class="notice bad">{problem}</p>{/if}

<section class="card">
	{#if !list}
		<p class="muted">{w.loading}</p>
	{:else if list.length === 0}
		<p class="muted">{w.noneConnected}</p>
	{:else}
		<table class="table">
			<thead>
				<tr>
					<th>{w.name}</th>
					<th class="hide-narrow">{w.key}</th>
					<th>{w.relay}</th>
					<th>{w.devices}</th>
				</tr>
			</thead>
			<tbody>
				{#each list as b (b.key + b.relay)}
					<tr>
						<td><strong>{b.name ?? '—'}</strong></td>
						<td class="hide-narrow mono muted" title={b.key}>{shortKey(b.key)}</td>
						<td class="secondary">{b.relay}</td>
						<td>{count(b.devices, ui.lang)}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</section>
