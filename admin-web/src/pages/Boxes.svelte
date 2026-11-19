<script lang="ts">
	import { onMount } from 'svelte';
	import { get, post, type BoxSummary } from '../lib/api';
	import { ago, shortKey } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';
	import Confirm from './Confirm.svelte';

	let { onchange }: { onchange: () => void } = $props();
	let q = $state('');
	let list = $state<BoxSummary[] | null>(null);
	let problem = $state<string | null>(null);
	let asking = $state<{ name: string; action: 'switch-off' | 'mark-stolen' } | null>(null);
	const w = $derived(ui.w);
	const now = () => Math.floor(Date.now() / 1000);

	async function load() {
		try {
			list = await get<BoxSummary[]>(`/boxes?q=${encodeURIComponent(q.trim())}`);
			problem = null;
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	async function act(name: string, action: string) {
		asking = null;
		try {
			await post(`/boxes/${encodeURIComponent(name)}/${action}`);
			await load();
			onchange();
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	let timer: ReturnType<typeof setTimeout>;
	function typed() {
		clearTimeout(timer);
		timer = setTimeout(load, 250);
	}

	onMount(load);
</script>

<div class="page-head">
	<h1>{w.boxes}</h1>
	<input class="input search" type="search" placeholder={w.search} bind:value={q} oninput={typed} />
</div>

{#if problem}<p class="notice bad">{problem}</p>{/if}

<section class="card">
	{#if !list}
		<p class="muted">{w.loading}</p>
	{:else if list.length === 0}
		<p class="muted">{w.noBoxes}</p>
	{:else}
		<table class="table">
			<thead>
				<tr>
					<th>{w.name}</th>
					<th class="hide-narrow">{w.key}</th>
					<th>{w.state}</th>
					<th class="hide-narrow">{w.since}</th>
					<th></th>
				</tr>
			</thead>
			<tbody>
				{#each list as b (b.name)}
					<tr>
						<td><strong>{b.name}</strong></td>
						<td class="hide-narrow mono muted" title={b.key}>{shortKey(b.key)}</td>
						<td>
							<div class="badges">
								{#if b.stolen}<span class="badge bad">{w.isStolen}</span>
								{:else if b.disabled}<span class="badge warn">{w.isOff}</span>
								{:else}<span class="badge good">{w.active}</span>{/if}
								{#if b.factory}<span class="badge accent">{w.factory}</span>{/if}
								{#if b.door_until > now()}<span class="badge">{w.doorOpen}</span>{/if}
								{#if !b.has_record && !b.stolen}<span class="badge">{w.noRecord}</span>{/if}
							</div>
						</td>
						<td class="hide-narrow muted">{ago(b.registered_at, now(), ui.lang)}</td>
						<td>
							<div class="actions">
								{#if !b.stolen}
									{#if b.disabled}
										<button class="btn small" onclick={() => act(b.name, 'switch-on')}>{w.switchOn}</button>
									{:else}
										<button class="btn small" onclick={() => (asking = { name: b.name, action: 'switch-off' })}>{w.switchOff}</button>
									{/if}
									<button class="btn small danger" onclick={() => (asking = { name: b.name, action: 'mark-stolen' })}>{w.markStolen}</button>
								{/if}
							</div>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</section>

{#if asking}
	{@const a = asking}
	<Confirm
		text={fill(a.action === 'switch-off' ? w.confirmSwitchOff : w.confirmStolen, { name: a.name })}
		action={a.action === 'switch-off' ? w.switchOff : w.markStolen}
		danger={a.action === 'mark-stolen'}
		onconfirm={() => act(a.name, a.action)}
		oncancel={() => (asking = null)}
	/>
{/if}
