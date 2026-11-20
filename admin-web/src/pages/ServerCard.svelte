<script lang="ts">
	// One server: how it's doing and, when known, what it moved this month.
	// On the Servers page for every server, and on a family relay's own page.
	import type { RelayHealth, ServerLine } from '../lib/api';
	import { ago, count, duration, size } from '../lib/format';
	import { ui } from '../lib/ui.svelte';
	import { fill } from '../lib/words';

	let {
		line,
		now,
		probe = undefined,
		onforget = undefined
	}: {
		line: Omit<ServerLine, 'month'> & { month: ServerLine['month'] | null };
		now: number;
		probe?: RelayHealth;
		onforget?: () => void;
	} = $props();
	const w = $derived(ui.w);
	const sz = (b: number) => size(b, ui.lang);
	const decimal = (v: number) => new Intl.NumberFormat(ui.lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(v);
	const st = $derived(line.stats);
	const pct = (used: number, total: number) => (total > 0 ? Math.round((used / total) * 100) : 0);
	const level = (p: number) => (p >= 90 ? 'bad' : p >= 75 ? 'warn' : '');
	const mem = $derived(pct(st.mem_used, st.mem_total));
	const disk = $derived(pct(st.disk_used, st.disk_total));

	function role(r: string): string {
		return r === 'control plane' ? w.controlPlane : r === 'relay' ? w.relay : r === 'second DNS' ? w.roleSecondDns : r;
	}
</script>

<section class="card stack">
	<div class="server-head">
		<h2><span class="dot {line.online ? 'up' : 'down'}"></span>{line.name}</h2>
		<span class="badge {line.online ? 'good' : 'bad'}">{line.online ? w.serverOnline : w.serverOffline}</span>
	</div>
	<div class="rows">
		<div class="row"><span class="secondary">{w.serverRole}</span><span>{role(st.role)} · {fill(w.version, { v: st.version })}</span></div>
		<div class="row"><span class="secondary">{w.lastReport}</span><span>{ago(line.last_seen, now, ui.lang)}</span></div>
		{#if line.online}
			<div class="row"><span class="secondary">{w.machineUp}</span><span>{duration(st.uptime, ui.lang)}</span></div>
			<div class="row"><span class="secondary">{w.programUp}</span><span>{duration(now - st.started, ui.lang)}</span></div>
			<div class="row">
				<span class="secondary">{w.load}</span>
				<span class:warn-text={st.load > st.cpus}>{fill(w.loadOf, { load: decimal(st.load), cpus: count(st.cpus, ui.lang) })}</span>
			</div>
			<div class="row">
				<div class="row-main grow">
					<div class="secondary">{w.memory}</div>
					<div class="meter {level(mem)}"><span style="width: {mem}%"></span></div>
				</div>
				<span>{fill(w.usedOf, { used: sz(st.mem_used), total: sz(st.mem_total) })}</span>
			</div>
			<div class="row">
				<div class="row-main grow">
					<div class="secondary">{w.disk}</div>
					<div class="meter {level(disk)}"><span style="width: {disk}%"></span></div>
				</div>
				<span>{fill(w.usedOf, { used: sz(st.disk_used), total: sz(st.disk_total) })}</span>
			</div>
		{/if}
		{#if line.month}
			<div class="row"><span class="secondary">{w.netInThisMonth}</span><strong>{sz(line.month.rx)}</strong></div>
			<div class="row"><span class="secondary">{w.netOutThisMonth}</span><strong>{sz(line.month.tx)}</strong></div>
		{/if}
		{#if st.role === 'relay'}
			{#if line.month}
				<div class="row"><span class="secondary">{w.relayUseThisMonth}</span><strong>{sz(line.month.relay)}</strong></div>
			{/if}
			{#if probe}
				<div class="row">
					<span class="secondary">{w.fromOutside}</span>
					<span class="badge {probe.up ? 'good' : 'bad'}">{probe.up ? `${w.relayUp} · ${probe.ms} ${w.ms}` : w.relayDown}</span>
				</div>
			{/if}
			<div class="row">
				<span class="secondary">{w.turnServer}</span>
				{#if st.turn === null}
					<span class="muted">{w.none}</span>
				{:else}
					<span class="badge {st.turn ? 'good' : 'bad'}">{st.turn ? w.relayUp : w.relayDown}</span>
				{/if}
			</div>
			{#if line.month && st.turn !== null}
				<div class="row"><span class="secondary">{w.turnUseThisMonth}</span><strong>{sz(line.month.turn)}</strong></div>
			{/if}
		{/if}
	</div>
	{#if !line.online && onforget}
		<div class="actions"><button class="btn small" onclick={onforget}>{w.forgetServer}</button></div>
	{/if}
</section>
