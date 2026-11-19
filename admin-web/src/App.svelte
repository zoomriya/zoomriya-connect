<script lang="ts">
	import { onMount } from 'svelte';
	import { SignedOut, get, setToken, signIn, token, type Overview } from './lib/api';
	import { ui } from './lib/ui.svelte';
	import Boxes from './pages/Boxes.svelte';
	import FactoryTokens from './pages/FactoryTokens.svelte';
	import OverviewPage from './pages/Overview.svelte';
	import RelayPage from './pages/Relay.svelte';
	import Reports from './pages/Reports.svelte';
	import SignIn from './pages/SignIn.svelte';

	type Page = 'overview' | 'boxes' | 'reports' | 'tokens';

	let overview = $state<Overview | null>(null);
	let page = $state<Page>('overview');
	let ready = $state(false);
	const w = $derived(ui.w);

	async function refresh() {
		try {
			overview = await get<Overview>('/overview');
		} catch (e) {
			if (e instanceof SignedOut) overview = null;
		}
	}

	async function onSignIn(t: string): Promise<string | null> {
		try {
			overview = await signIn(t);
			return null;
		} catch (e) {
			setToken(null);
			return e instanceof SignedOut ? w.wrongToken : w.cantReach;
		}
	}

	function signOut() {
		setToken(null);
		overview = null;
		page = 'overview';
	}

	onMount(() => {
		(async () => {
			if (token()) await refresh();
			ready = true;
		})();
		const tick = setInterval(() => overview && refresh(), 30_000);
		return () => clearInterval(tick);
	});

	const isPlane = $derived(overview?.kind === 'control_plane');
	const nav = $derived<{ id: Page; label: string; count?: number }[]>(
		overview?.kind === 'control_plane'
			? [
					{ id: 'overview', label: w.overview },
					{ id: 'boxes', label: w.boxes },
					{ id: 'reports', label: w.reports, count: overview.reports_open || undefined },
					{ id: 'tokens', label: w.factoryTokens }
				]
			: [{ id: 'overview', label: w.overview }]
	);
</script>

{#if !ready}
	<div class="signin"><p class="muted">{w.loading}</p></div>
{:else if !overview}
	<SignIn onsubmit={onSignIn} />
{:else}
	<div class="shell">
		<aside class="side">
			<div class="brand">
				<div class="brand-mark" aria-hidden="true">Z</div>
				<div>
					<div class="brand-name">{w.product}</div>
					<div class="brand-kind">{isPlane ? w.controlPlane : w.relay}</div>
				</div>
			</div>
			<nav aria-label={w.product}>
				{#each nav as item (item.id)}
					<button
						class="nav-item"
						aria-current={page === item.id ? 'page' : undefined}
						onclick={() => (page = item.id)}
					>
						<span>{item.label}</span>
						{#if item.count}<span class="count">{item.count}</span>{/if}
					</button>
				{/each}
			</nav>
			<div class="side-foot">
				<div>
					<div class="side-label">{w.theme}</div>
					<div class="seg">
						<button aria-pressed={ui.theme === 'midnight'} onclick={() => ui.setTheme('midnight')}>{w.midnight}</button>
						<button aria-pressed={ui.theme === 'daylight'} onclick={() => ui.setTheme('daylight')}>{w.daylight}</button>
					</div>
				</div>
				<div>
					<div class="side-label">{w.language}</div>
					<div class="seg">
						<button aria-pressed={ui.lang === 'en'} onclick={() => ui.setLang('en')}>English</button>
						<button aria-pressed={ui.lang === 'fr'} onclick={() => ui.setLang('fr')}>Français</button>
					</div>
				</div>
				<button class="btn small" onclick={signOut}>{w.signOut}</button>
			</div>
		</aside>
		<main class="main">
			{#if overview.kind === 'relay'}
				<RelayPage {overview} onrefresh={refresh} />
			{:else if page === 'overview'}
				<OverviewPage {overview} onrefresh={refresh} />
			{:else if page === 'boxes'}
				<Boxes onchange={refresh} />
			{:else if page === 'reports'}
				<Reports onchange={refresh} />
			{:else}
				<FactoryTokens {overview} onchange={refresh} />
			{/if}
		</main>
	</div>
{/if}
