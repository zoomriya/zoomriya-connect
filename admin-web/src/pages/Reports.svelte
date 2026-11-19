<script lang="ts">
	import { onMount } from 'svelte';
	import { get, post, type Report } from '../lib/api';
	import { ago } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';
	import Confirm from './Confirm.svelte';

	let { onchange }: { onchange: () => void } = $props();
	let list = $state<Report[] | null>(null);
	let problem = $state<string | null>(null);
	let asking = $state<string | null>(null);
	const w = $derived(ui.w);
	const now = () => Math.floor(Date.now() / 1000);

	async function load() {
		try {
			list = await get<Report[]>('/reports');
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	async function run(path: string) {
		try {
			await post(path);
			await load();
			onchange();
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	onMount(load);
</script>

<div class="page-head"><h1>{w.reports}</h1></div>

{#if problem}<p class="notice bad">{problem}</p>{/if}

<section class="card">
	{#if !list}
		<p class="muted">{w.loading}</p>
	{:else if list.length === 0}
		<p class="muted">{w.noReports}</p>
	{:else}
		<div class="rows">
			{#each list as r (r.id)}
				<div class="row">
					<div class="row-main">
						<div class="row-title">{r.name}</div>
						<div class="row-sub">{w.reported} {ago(r.at, now(), ui.lang)}</div>
						<p class="secondary gap-top">{r.reason}</p>
					</div>
					<div class="actions">
						<button class="btn small danger" onclick={() => (asking = r.name)}>{w.switchOffName}</button>
						<button class="btn small" onclick={() => run(`/reports/${r.id}/done`)}>{w.done}</button>
					</div>
				</div>
			{/each}
		</div>
	{/if}
</section>

{#if asking}
	{@const name = asking}
	<Confirm
		text={fill(w.confirmSwitchOff, { name })}
		action={w.switchOff}
		onconfirm={() => {
			asking = null;
			run(`/boxes/${encodeURIComponent(name)}/switch-off`);
		}}
		oncancel={() => (asking = null)}
	/>
{/if}
