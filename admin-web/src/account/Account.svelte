<script lang="ts">
	// The families' account page: sign in with the ZOOMRIYA Connect account an
	// operator gave them, see their boxes and this month's relay use, and
	// change the password. The sign-in lives in this tab's session storage only.
	import { onMount } from 'svelte';
	import { size } from '../lib/format';
	import { fill, pickLang, type Lang } from '../lib/words';
	import { accountWords } from './words';

	interface Box {
		name: string;
		fqdn: string;
		connected: boolean;
		devices: number;
		used: number;
		allowance: number;
		slowed: boolean;
	}
	interface Me {
		username: string;
		max_boxes: number;
		boxes: Box[];
	}

	const KEY = 'zoomriya-connect-account-token';
	const stored = (k: string) => {
		try {
			return localStorage.getItem(k);
		} catch {
			return null;
		}
	};
	const keep = (k: string, v: string) => {
		try {
			localStorage.setItem(k, v);
		} catch {
			/* not kept: fine */
		}
	};
	let lang = $state<Lang>(
		stored('zoomriya-connect-account-lang') === 'fr' || stored('zoomriya-connect-account-lang') === 'en'
			? (stored('zoomriya-connect-account-lang') as Lang)
			: pickLang(navigator.languages ?? [])
	);
	let theme = $state<'midnight' | 'daylight'>(stored('zoomriya-connect-account-theme') === 'daylight' ? 'daylight' : 'midnight');
	const w = $derived(accountWords[lang]);
	$effect(() => {
		document.documentElement.classList.toggle('midnight', theme === 'midnight');
		document.documentElement.lang = lang;
	});

	let token = $state<string | null>(null);
	let me = $state<Me | null>(null);
	let ready = $state(false);
	let username = $state('');
	let password = $state('');
	let problem = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);
	let current = $state('');
	let next = $state('');
	let again = $state('');

	function setToken(t: string | null) {
		token = t;
		try {
			if (t) sessionStorage.setItem(KEY, t);
			else sessionStorage.removeItem(KEY);
		} catch {
			/* private window: lasts until the page closes */
		}
	}

	async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
		const res = await fetch(`/v1/account${path}`, {
			method,
			headers: {
				...(token ? { authorization: `Bearer ${token}` } : {}),
				...(body === undefined ? {} : { 'content-type': 'application/json' })
			},
			body: body === undefined ? undefined : JSON.stringify(body)
		});
		if (res.status === 204) return undefined as T;
		const data = await res.json().catch(() => ({}));
		if (res.status === 401 && path === '') {
			setToken(null);
			me = null;
		}
		if (!res.ok) throw new Error(data.message ?? w.cantReach);
		return data as T;
	}

	async function load() {
		try {
			me = await call<Me>('GET', '');
		} catch {
			me = null;
		}
	}

	async function signIn(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		problem = null;
		try {
			const r = await call<{ token: string }>('POST', '/sign-in', { username: username.trim(), password });
			setToken(r.token);
			password = '';
			notice = null;
			await load();
		} catch (err) {
			problem = (err as Error).message;
		}
		busy = false;
	}

	async function signOut() {
		try {
			await call('POST', '/sign-out');
		} catch {
			/* signed out here anyway */
		}
		setToken(null);
		me = null;
	}

	async function changePassword(e: SubmitEvent) {
		e.preventDefault();
		problem = null;
		if (next !== again) {
			problem = w.notSame;
			return;
		}
		busy = true;
		try {
			await call('POST', '/password', { password: current, new_password: next });
			current = next = again = '';
			setToken(null);
			me = null;
			notice = w.changed;
		} catch (err) {
			problem = (err as Error).message;
		}
		busy = false;
	}

	function use(b: Box): string {
		const used = size(b.used, lang);
		return b.allowance > 0 ? fill(w.usedOf, { used, allowance: size(b.allowance, lang) }) : fill(w.used, { used });
	}

	onMount(() => {
		(async () => {
			try {
				token = sessionStorage.getItem(KEY);
			} catch {
				token = null;
			}
			if (token) await load();
			ready = true;
		})();
	});
</script>

{#snippet choices()}
	<div class="account-choices">
		<div class="seg" role="group" aria-label={w.theme}>
			<button aria-pressed={theme === 'midnight'} onclick={() => { theme = 'midnight'; keep('zoomriya-connect-account-theme', theme); }}>{w.midnight}</button>
			<button aria-pressed={theme === 'daylight'} onclick={() => { theme = 'daylight'; keep('zoomriya-connect-account-theme', theme); }}>{w.daylight}</button>
		</div>
		<div class="seg" role="group" aria-label={w.language}>
			<button aria-pressed={lang === 'en'} onclick={() => { lang = 'en'; keep('zoomriya-connect-account-lang', lang); }}>English</button>
			<button aria-pressed={lang === 'fr'} onclick={() => { lang = 'fr'; keep('zoomriya-connect-account-lang', lang); }}>Français</button>
		</div>
	</div>
{/snippet}

{#if !ready}
	<div class="signin"><p class="muted">{w.loading}</p></div>
{:else if !me}
	<div class="signin">
		<form class="card" onsubmit={signIn}>
			<div class="brand flush">
				<div class="brand-mark" aria-hidden="true">Z</div>
				<div class="brand-name">{w.product}</div>
			</div>
			<h1>{w.title}</h1>
			<p class="secondary">{w.signInHelp}</p>
			{#if notice}<p class="notice good" role="status">{notice}</p>{/if}
			<label class="field">
				<span>{w.username}</span>
				<input class="input" type="text" autocomplete="username" spellcheck="false" bind:value={username} />
			</label>
			<label class="field">
				<span>{w.password}</span>
				<input class="input" type="password" autocomplete="current-password" bind:value={password} />
			</label>
			{#if problem}<p class="notice bad" role="alert">{problem}</p>{/if}
			<button class="btn primary" type="submit" disabled={busy || !username.trim() || !password}>{w.signIn}</button>
			{@render choices()}
		</form>
	</div>
{:else}
	<main class="main account-main">
		<div class="page-head">
			<div class="brand flush">
				<div class="brand-mark" aria-hidden="true">Z</div>
				<div>
					<div class="brand-name">{w.product}</div>
					<div class="brand-kind">{me.username}</div>
				</div>
			</div>
			<button class="btn" onclick={signOut}>{w.signOut}</button>
		</div>

		<div class="stack">
			<section class="card">
				<h2>{w.boxes}</h2>
				<p class="muted">{fill(w.boxesOf, { n: me.boxes.length, max: me.max_boxes })}</p>
				<div class="rows">
					{#each me.boxes as b (b.name)}
						<div class="row">
							<div class="row-main">
								<div class="row-title"><span class="dot {b.connected ? 'up' : 'idle'}"></span>{b.fqdn}</div>
								<div class="row-sub">{use(b)}</div>
								{#if b.slowed}<div class="row-sub warn-text">{w.slowed}</div>{/if}
							</div>
							<span class="badge {b.connected ? 'good' : ''}">
								{b.connected ? (b.devices ? fill(w.devices, { n: b.devices }) : w.connected) : w.notConnected}
							</span>
						</div>
					{:else}
						<p class="secondary">{w.noBoxes}</p>
					{/each}
				</div>
			</section>

			<section class="card">
				<h2>{w.changePassword}</h2>
				<form class="stack" onsubmit={changePassword}>
					<label class="field">
						<span>{w.currentPassword}</span>
						<input class="input" type="password" autocomplete="current-password" bind:value={current} />
					</label>
					<label class="field">
						<span>{w.newPassword}</span>
						<input class="input" type="password" autocomplete="new-password" bind:value={next} />
					</label>
					<label class="field">
						<span>{w.repeatPassword}</span>
						<input class="input" type="password" autocomplete="new-password" bind:value={again} />
					</label>
					<p class="muted">{w.passwordRule}</p>
					{#if problem}<p class="notice bad" role="alert">{problem}</p>{/if}
					<div class="actions">
						<button class="btn primary" type="submit" disabled={busy || !current || next.length < 10 || !again}>{w.change}</button>
					</div>
				</form>
			</section>
			{@render choices()}
		</div>
	</main>
{/if}
