<script lang="ts">
	import { onMount } from 'svelte';
	import { get, post, type Allowances, type BoxSummary } from '../lib/api';
	import { ago, shortKey, size } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';
	import BoxAllowance from './BoxAllowance.svelte';
	import Confirm from './Confirm.svelte';

	let { onchange, allowances }: { onchange: () => void; allowances: Allowances } = $props();
	let editing = $state<BoxSummary | null>(null);
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

	async function setAllowance(name: string, gb: number | null) {
		editing = null;
		try {
			await post(`/boxes/${encodeURIComponent(name)}/allowance`, { gb });
			await load();
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	// Relay and TURN together, as the allowance counts them.
	function usage(b: BoxSummary): string {
		const used = size(b.used + b.turn, ui.lang);
		return b.allowance_gb === 0 ? used : fill(w.ofAllowance, { used, allowance: size(b.allowance_gb * 1e9, ui.lang) });
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
					<th class="hide-narrow">{w.account}</th>
					<th class="hide-narrow">{w.key}</th>
					<th>{w.state}</th>
					<th>{w.thisMonth}</th>
					<th class="hide-narrow">{w.since}</th>
					<th></th>
				</tr>
			</thead>
			<tbody>
				{#each list as b (b.name)}
					<tr>
						<td><strong>{b.name}</strong></td>
						<td class="hide-narrow secondary">{b.account ?? w.noAccount}</td>
						<td class="hide-narrow mono muted" title={b.key}>{shortKey(b.key)}</td>
						<td>
							<div class="badges">
								{#if b.stolen}<span class="badge bad">{w.isStolen}</span>
								{:else if b.disabled}<span class="badge warn">{w.isOff}</span>
								{:else}<span class="badge good">{w.active}</span>{/if}
								{#if b.factory}<span class="badge accent">{w.factory}</span>{/if}
								{#if b.door_until > now()}<span class="badge">{w.doorOpen}</span>{/if}
								{#if !b.has_record && !b.stolen}<span class="badge">{w.noRecord}</span>{/if}
								{#if b.relay}<span class="badge good" title={b.relay}>{w.isConnected}</span>{/if}
							</div>
						</td>
						<td>
							<div class="badges">
								<span class="secondary" title={fill(w.relayAndTurn, { relay: size(b.used, ui.lang), turn: size(b.turn, ui.lang) })}>{usage(b)}</span>
								{#if b.slowed}<span class="badge warn">{w.slowed}</span>{/if}
								{#if b.own_allowance_gb !== null}<span class="badge accent">{w.ownAllowance}</span>{/if}
							</div>
						</td>
						<td class="hide-narrow muted">{ago(b.registered_at, now(), ui.lang)}</td>
						<td>
							<div class="actions">
								<button class="btn small" onclick={() => (editing = b)}>{w.setAllowance}</button>
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

{#if editing}
	{@const b = editing}
	<BoxAllowance
		box={b}
		usual={b.factory ? allowances.factory_gb : allowances.default_gb}
		onsave={(gb) => setAllowance(b.name, gb)}
		oncancel={() => (editing = null)}
	/>
{/if}
