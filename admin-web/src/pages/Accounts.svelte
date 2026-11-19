<script lang="ts">
	import { onMount } from 'svelte';
	import { get, post, type Account } from '../lib/api';
	import { ago } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';
	import Confirm from './Confirm.svelte';
	import Password from './Password.svelte';

	let { onchange }: { onchange: () => void } = $props();
	let q = $state('');
	let list = $state<Account[] | null>(null);
	let problem = $state<string | null>(null);
	let username = $state('');
	let maxBoxes = $state(3);
	let busy = $state(false);
	let asking = $state<{ name: string; action: 'block' | 'reset-password' } | null>(null);
	let shown = $state<{ username: string; password: string } | null>(null);
	const w = $derived(ui.w);
	const now = () => Math.floor(Date.now() / 1000);

	async function load() {
		try {
			list = await get<Account[]>(`/accounts?q=${encodeURIComponent(q.trim())}`);
			problem = null;
		} catch (e) {
			problem = String((e as Error).message);
		}
	}

	async function create(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		try {
			shown = await post<{ username: string; password: string }>('/accounts', {
				username: username.trim(),
				max_boxes: maxBoxes
			});
			username = '';
			await load();
			onchange();
		} catch (err) {
			problem = String((err as Error).message);
		}
		busy = false;
	}

	async function act(name: string, action: string, body?: unknown) {
		asking = null;
		try {
			const r = await post<{ username: string; password: string } | undefined>(
				`/accounts/${encodeURIComponent(name)}/${action}`,
				body
			);
			if (action === 'reset-password' && r) shown = r;
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
	<h1>{w.accounts}</h1>
	<input class="input search" type="search" placeholder={w.searchAccounts} bind:value={q} oninput={typed} />
</div>

<div class="stack">
	<section class="card stack">
		<p class="secondary">{w.accountsHelp}</p>
		<form class="new-account" onsubmit={create}>
			<label class="field">
				<span>{w.username}</span>
				<input class="input" type="text" autocomplete="off" spellcheck="false" bind:value={username} />
			</label>
			<label class="field">
				<span>{w.maxBoxes}</span>
				<input class="input narrow" type="number" min="1" max="100" step="1" bind:value={maxBoxes} />
			</label>
			<button class="btn primary" type="submit" disabled={busy || username.trim().length < 3}>{w.newAccount}</button>
		</form>
	</section>

	{#if problem}<p class="notice bad">{problem}</p>{/if}

	<section class="card">
		{#if !list}
			<p class="muted">{w.loading}</p>
		{:else if list.length === 0}
			<p class="muted">{w.noAccounts}</p>
		{:else}
			<table class="table">
				<thead>
					<tr>
						<th>{w.username}</th>
						<th>{w.boxes}</th>
						<th>{w.state}</th>
						<th class="hide-narrow">{w.since}</th>
						<th></th>
					</tr>
				</thead>
				<tbody>
					{#each list as a (a.username)}
						<tr>
							<td><strong>{a.username}</strong></td>
							<td>
								<div class="badges">
									<span class="secondary">{fill(w.boxesOf, { n: a.boxes.length, max: a.max_boxes })}</span>
									{#each a.boxes as b (b)}<span class="badge">{b}</span>{/each}
								</div>
							</td>
							<td>
								<div class="badges">
									{#if a.blocked}<span class="badge bad">{w.blocked}</span>
									{:else}<span class="badge good">{w.active}</span>{/if}
									{#if a.locked_until > now()}<span class="badge warn">{w.locked}</span>{/if}
								</div>
							</td>
							<td class="hide-narrow muted">{ago(a.created_at, now(), ui.lang)}</td>
							<td>
								<div class="actions">
									<select
										class="input narrow"
										aria-label={w.maxBoxes}
										value={a.max_boxes}
										onchange={(e) => act(a.username, 'max-boxes', { max_boxes: Number((e.target as HTMLSelectElement).value) })}
									>
										{#each [1, 2, 3, 4, 5, 10, 20] as n (n)}<option value={n}>{n}</option>{/each}
									</select>
									<button class="btn small" onclick={() => (asking = { name: a.username, action: 'reset-password' })}>{w.resetPassword}</button>
									{#if a.blocked}
										<button class="btn small" onclick={() => act(a.username, 'unblock')}>{w.unblock}</button>
									{:else}
										<button class="btn small danger" onclick={() => (asking = { name: a.username, action: 'block' })}>{w.block}</button>
									{/if}
								</div>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</section>
</div>

{#if asking}
	{@const a = asking}
	<Confirm
		text={fill(a.action === 'block' ? w.confirmBlock : w.confirmReset, { name: a.name })}
		action={a.action === 'block' ? w.block : w.resetPassword}
		danger={a.action === 'block'}
		onconfirm={() => act(a.name, a.action)}
		oncancel={() => (asking = null)}
	/>
{/if}

{#if shown}
	<Password username={shown.username} password={shown.password} onclose={() => (shown = null)} />
{/if}
