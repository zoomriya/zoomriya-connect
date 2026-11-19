<script lang="ts">
	import type { ControlPlaneOverview } from '../lib/api';
	import { ago, count } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';

	let { overview, onrefresh }: { overview: ControlPlaneOverview; onrefresh: () => void } = $props();
	const w = $derived(ui.w);
	const n = (v: number) => count(v, ui.lang);
	const now = () => Math.floor(Date.now() / 1000);
</script>

<div class="page-head">
	<div>
		<h1>{w.overview}</h1>
		<p class="muted">{overview.zone} · {fill(w.version, { v: overview.version })}</p>
	</div>
	<button class="btn" onclick={onrefresh}>{w.refresh}</button>
</div>

<div class="stack">
	<div class="grid stats">
		<div class="card stat"><div class="stat-label">{w.registered}</div><div class="stat-value">{n(overview.boxes.all)}</div></div>
		<div class="card stat"><div class="stat-label">{w.newToday}</div><div class="stat-value">{n(overview.new.day)}</div></div>
		<div class="card stat"><div class="stat-label">{w.newThisWeek}</div><div class="stat-value">{n(overview.new.week)}</div></div>
		<div class="card stat" class:warn={overview.reports_open > 0}>
			<div class="stat-label">{w.openReports}</div><div class="stat-value">{n(overview.reports_open)}</div>
		</div>
		<div class="card stat"><div class="stat-label">{w.withFactoryToken}</div><div class="stat-value">{n(overview.boxes.factory)}</div></div>
		<div class="card stat"><div class="stat-label">{w.unusedTokens}</div><div class="stat-value">{n(overview.factory_tokens.unused)}</div></div>
		<div class="card stat"><div class="stat-label">{w.stolen}</div><div class="stat-value">{n(overview.boxes.stolen)}</div></div>
		<div class="card stat"><div class="stat-label">{w.switchedOff}</div><div class="stat-value">{n(overview.boxes.off)}</div></div>
	</div>

	<div class="grid two">
		<section class="card">
			<h2>{w.relays}</h2>
			<div class="rows">
				{#each overview.relays as r (r.url)}
					<div class="row">
						<div class="row-main">
							<div class="row-title"><span class="dot {r.up ? 'up' : 'down'}"></span>{r.url.replace(/^https:\/\//, '')}</div>
							<div class="row-sub">{ago(r.checked, now(), ui.lang)}</div>
						</div>
						<span class="badge {r.up ? 'good' : 'bad'}">{r.up ? `${w.relayUp} · ${r.ms} ${w.ms}` : w.relayDown}</span>
					</div>
				{:else}
					<p class="muted"><span class="dot idle"></span>{w.notCheckedYet}</p>
				{/each}
			</div>
		</section>
		<section class="card">
			<h2>{w.guests}</h2>
			<div class="rows">
				<div class="row"><span class="secondary">{w.boxesForSharing}</span><strong>{n(overview.setup_messages.boxes)}</strong></div>
				<div class="row"><span class="secondary">{w.guestsSettingUp}</span><strong>{n(overview.setup_messages.guests)}</strong></div>
				<div class="row">
					<span class="secondary">{w.guestRelay}</span>
					<span class="badge {overview.guest_relay ? 'good' : ''}">{overview.guest_relay ? w.on : w.off}</span>
				</div>
			</div>
		</section>
	</div>
</div>
