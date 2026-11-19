<script lang="ts">
	// The toolbar button's popup: pairing this laptop with the box at home,
	// then opening the box.
	import { forgetBox, homeFor, isFirefox, loadBox, permissionFor, saveBox, type Box, type IceServer } from '../lib/box';
	import { devicePublic, forgetKeys, linkPublic, linkSig, newKeys, openSealed, peekSealed } from '../lib/keys';
	import { pickLang, words } from '../lib/words';
	import { ext } from '../lib/ext';

	interface Paired {
		box_key: string;
		fqdn: string;
		device: string;
		pass: string;
		until: number;
		server: string;
		relays?: string[];
		ice?: IceServer[];
		desktop?: string;
		tls_keys?: string[];
	}

	const w = words[pickLang(navigator.languages ?? [])];
	document.documentElement.lang = pickLang(navigator.languages ?? []);
	let box = $state<Box | null>(null);
	let ready = $state(false);
	let name = $state('');
	let code = $state('');
	let laptop = $state('');
	let problem = $state<string | null>(null);
	let busy = $state(false);
	let forgetting = $state(false);

	loadBox().then((b) => {
		box = b;
		ready = true;
	});

	async function pair(e: SubmitEvent) {
		e.preventDefault();
		problem = null;
		const home = homeFor(name);
		if (!home) {
			problem = w.badName;
			return;
		}
		// Asked first, while the click still counts as the person's.
		const granted = await ext.permissions.request({ origins: [permissionFor(home)] });
		if (!granted) {
			problem = w.noPermission;
			return;
		}
		busy = true;
		try {
			const keys = await newKeys();
			const body = {
				code: code.trim(),
				device_key: await devicePublic(keys),
				link: await linkPublic(keys),
				link_sig: await linkSig(keys),
				name: laptop.trim() || w.laptopNameHint
			};
			let res: Response;
			try {
				res = await fetch(`${home}/laptop/pair`, {
					method: 'POST',
					headers: { 'content-type': 'application/json' },
					body: JSON.stringify(body)
				});
			} catch {
				throw new Error(w.unreachable);
			}
			const got = await res.json().catch(() => ({}));
			if (!res.ok) throw new Error(got.message ?? w.unreachable);
			// The box seals what it sends with its key; that key is pinned from now on.
			const named = peekSealed<Paired>(got.paired ?? '');
			const paired = named && (await openSealed<Paired>(got.paired, 'laptop-paired/v1', named.box_key));
			if (!paired) throw new Error(w.notTheBox);
			const b: Box = {
				fqdn: paired.fqdn,
				boxKey: paired.box_key,
				device: paired.device,
				pass: paired.pass,
				until: paired.until,
				server: paired.server,
				relays: paired.relays ?? [],
				ice: paired.ice ?? [],
				desktop: paired.desktop || `https://${paired.fqdn}:7443`,
				tlsKeys: paired.tls_keys ?? [],
				home,
				name: body.name
			};
			await saveBox(b);
			// Firefox: the box's service worker goes in now, while at home.
			if (isFirefox()) await ext.runtime.sendMessage({ t: 'setup' }).catch(() => null);
			box = b;
			code = '';
		} catch (err) {
			problem = (err as Error).message;
		}
		busy = false;
	}

	async function open() {
		problem = null;
		if (box && isFirefox()) {
			// Firefox opens Box Desktop's own address, answered through the
			// box's service worker (docs/laptops.md, "Firefox").
			const got = await ext.runtime.sendMessage({ t: 'setup', wait: 8000 }).catch(() => null);
			if (!got?.ready) {
				problem = w.notSetUp;
				return;
			}
			void ext.tabs.create({ url: `${box.desktop}/` });
		} else {
			void ext.tabs.create({ url: ext.runtime.getURL('zr/open.html') });
		}
		window.close();
	}

	async function forget() {
		await forgetKeys();
		await forgetBox();
		box = null;
		forgetting = false;
	}
</script>

<main>
	<div class="brand">
		<img src="/zr/icons/48.png" alt="" />
		<b>{w.product}</b>
	</div>
	{#if !ready}
		<p class="muted">…</p>
	{:else if box}
		<div class="box">
			<span class="muted">{w.paired}</span>
			<b>{box.fqdn}</b>
		</div>
		<button class="primary" onclick={open}>{w.open}</button>
		{#if problem}<p class="notice bad" role="alert">{problem}</p>{/if}
		{#if forgetting}
			<p>{w.forgetSure}</p>
			<div class="row">
				<button onclick={() => (forgetting = false)}>{w.cancel}</button>
				<button class="primary" onclick={forget}>{w.forgetYes}</button>
			</div>
		{:else}
			<button class="danger" onclick={() => (forgetting = true)}>{w.forget}</button>
		{/if}
	{:else}
		<h1>{w.pairTitle}</h1>
		<p class="muted">{w.pairHelp}</p>
		<form onsubmit={pair}>
			<main class="flush">
				<label>{w.boxName}<input type="text" bind:value={name} placeholder={w.boxNameHint} spellcheck="false" autocomplete="off" /></label>
				<label>{w.code}<input class="code" type="text" bind:value={code} placeholder="XXXX-XXXX" spellcheck="false" autocomplete="off" /></label>
				<label>{w.laptopName}<input type="text" bind:value={laptop} placeholder={w.laptopNameHint} autocomplete="off" /></label>
				{#if problem}<p class="notice bad" role="alert">{problem}</p>{/if}
				<button class="primary" type="submit" disabled={busy || !name.trim() || code.replace(/[^a-z0-9]/gi, '').length < 8}>
					{busy ? w.pairing : w.pair}
				</button>
				<p class="muted">{w.weaker}</p>
			</main>
		</form>
	{/if}
</main>
