<script lang="ts">
	import type { ControlPlaneOverview } from '../lib/api';
	import { count, size } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';

	let { overview, onrefresh }: { overview: ControlPlaneOverview; onrefresh: () => void } = $props();
	const w = $derived(ui.w);
	const n = (v: number) => count(v, ui.lang);
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
		<div class="card stat"><div class="stat-label">{w.accountsStat}</div><div class="stat-value">{n(overview.accounts.all)}</div></div>
		<div class="card stat"><div class="stat-label">{w.registered}</div><div class="stat-value">{n(overview.boxes.all)}</div></div>
		<div class="card stat"><div class="stat-label">{w.connectedBoxes}</div><div class="stat-value">{n(overview.connected.boxes)}</div></div>
		<div class="card stat"><div class="stat-label">{w.connectedDevices}</div><div class="stat-value">{n(overview.connected.devices)}</div></div>
		<div class="card stat" class:warn={overview.servers.online < overview.servers.all}>
			<div class="stat-label">{w.serversStat}</div>
			<div class="stat-value">{fill(w.nOfAll, { n: n(overview.servers.online), all: n(overview.servers.all) })}</div>
		</div>
		<div class="card stat"><div class="stat-label">{w.relayUseThisMonth}</div><div class="stat-value">{size(overview.relay_use.bytes, ui.lang)}</div></div>
		<div class="card stat"><div class="stat-label">{w.turnUseThisMonth}</div><div class="stat-value">{size(overview.relay_use.turn, ui.lang)}</div></div>
		<div class="card stat"><div class="stat-label">{w.newToday}</div><div class="stat-value">{n(overview.new.day)}</div></div>
		<div class="card stat"><div class="stat-label">{w.newThisWeek}</div><div class="stat-value">{n(overview.new.week)}</div></div>
		<div class="card stat" class:warn={overview.reports_open > 0}>
			<div class="stat-label">{w.openReports}</div><div class="stat-value">{n(overview.reports_open)}</div>
		</div>
		<div class="card stat"><div class="stat-label">{w.withFactoryToken}</div><div class="stat-value">{n(overview.boxes.factory)}</div></div>
		<div class="card stat"><div class="stat-label">{w.unusedTokens}</div><div class="stat-value">{n(overview.factory_tokens.unused)}</div></div>
		<div class="card stat"><div class="stat-label">{w.stolen}</div><div class="stat-value">{n(overview.boxes.stolen)}</div></div>
		<div class="card stat"><div class="stat-label">{w.switchedOff}</div><div class="stat-value">{n(overview.boxes.off)}</div></div>
		<div class="card stat" class:warn={overview.accounts.blocked > 0}><div class="stat-label">{w.blockedStat}</div><div class="stat-value">{n(overview.accounts.blocked)}</div></div>
	</div>

	<div class="grid two">
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
