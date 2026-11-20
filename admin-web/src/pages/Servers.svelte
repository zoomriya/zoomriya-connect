<script lang="ts">
	// Every ZOOMRIYA Connect server: the control plane, each relay with the
	// TURN server beside it, and the second DNS server. How each is doing,
	// and what it moved this month.
	import { onMount } from 'svelte';
	import { get, post, type ControlPlaneOverview, type ServerLine, type ServersPage } from '../lib/api';
	import { count, size } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';
	import Confirm from './Confirm.svelte';
	import ServerCard from './ServerCard.svelte';

	let { overview }: { overview: ControlPlaneOverview } = $props();
	let data = $state<ServersPage | null>(null);
	let problem = $state<string | null>(null);
	let forgetting = $state<string | null>(null);
	const w = $derived(ui.w);
	const sz = (b: number) => size(b, ui.lang);

	async function load() {
		try {
			data = await get<ServersPage>('/servers');
			problem = null;
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	async function forget(name: string) {
		forgetting = null;
		try {
			await post(`/servers/${encodeURIComponent(name)}/forget`, {});
			await load();
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	onMount(() => {
		load();
		const tick = setInterval(load, 15_000);
		return () => clearInterval(tick);
	});

	/** The control plane's own check of a relay from outside, if it has one. */
	function probe(s: ServerLine) {
		return overview.relays.find((r) => r.url.replace(/^https:\/\//, '').replace(/[/:].*$/, '') === s.name);
	}

	const servers = $derived(data?.servers ?? []);
	const online = $derived(servers.filter((s) => s.online).length);
	// Relays the control plane checks from outside that have never reported.
	const silent = $derived(
		overview.relays.filter((r) => !servers.some((s) => probe(s)?.url === r.url))
	);
</script>

<div class="page-head">
	<div>
		<h1>{w.servers}</h1>
		{#if data}<p class="muted">{fill(w.serversOnline, { n: count(online, ui.lang), all: count(servers.length, ui.lang) })}</p>{/if}
	</div>
	<button class="btn" onclick={load}>{w.refresh}</button>
</div>

{#if problem}<p class="notice bad">{problem}</p>{/if}

{#if !data}
	<section class="card"><p class="muted">{w.loading}</p></section>
{:else}
	<div class="stack">
		<div class="grid stats">
			<div class="card stat"><div class="stat-label">{w.netInThisMonth}</div><div class="stat-value">{sz(data.totals.rx)}</div></div>
			<div class="card stat"><div class="stat-label">{w.netOutThisMonth}</div><div class="stat-value">{sz(data.totals.tx)}</div></div>
			<div class="card stat"><div class="stat-label">{w.relayUseThisMonth}</div><div class="stat-value">{sz(data.totals.relay)}</div></div>
			<div class="card stat"><div class="stat-label">{w.turnUseThisMonth}</div><div class="stat-value">{sz(data.totals.turn)}</div></div>
		</div>
		<p class="muted">{w.serversHelp}</p>

		{#if servers.length === 0}
			<section class="card"><p class="muted">{w.noServersYet}</p></section>
		{/if}
		{#if silent.length}
			<section class="card">
				<h2>{w.neverReported}</h2>
				<div class="rows">
					{#each silent as r (r.url)}
						<div class="row">
							<div class="row-title"><span class="dot {r.up ? 'up' : 'down'}"></span>{r.url.replace(/^https:\/\//, '')}</div>
							<span class="badge {r.up ? 'good' : 'bad'}">{r.up ? `${w.relayUp} · ${r.ms} ${w.ms}` : w.relayDown}</span>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		<div class="grid two">
			{#each servers as s (s.name)}
				<ServerCard line={s} now={data.now} probe={probe(s)} onforget={() => (forgetting = s.name)} />
			{/each}
		</div>
	</div>
{/if}

{#if forgetting}
	<Confirm
		text={fill(w.forgetServerSure, { name: forgetting })}
		action={w.forgetServer}
		onconfirm={() => forgetting && forget(forgetting)}
		oncancel={() => (forgetting = null)}
	/>
{/if}
