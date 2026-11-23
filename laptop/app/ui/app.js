// The pairing window: the short code from Box Desktop, or forgetting the box.
// The words come from the app, in the laptop's language.
const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
let words = {};

async function show() {
	const info = await invoke('info');
	words = info.words;
	document.documentElement.lang = navigator.language.startsWith('fr') ? 'fr' : 'en';
	$('title').textContent = words.title;
	$('lead').textContent = words.lead;
	$('go').textContent = words.pair_button;
	$('paired-lead').textContent = words.paired_lead;
	$('forget').textContent = words.forget_button;
	$('pairing').hidden = info.paired;
	$('paired').hidden = !info.paired;
	if (!info.paired) $('code').focus();
}

$('pairing').addEventListener('submit', async (e) => {
	e.preventDefault();
	const code = $('code').value.trim();
	if (code.replace(/[^0-9a-z]/gi, '').length < 8) return;
	$('error').textContent = '';
	$('go').disabled = true;
	$('go').textContent = words.pairing;
	try {
		await invoke('pair', { code });
		$('code').value = '';
		window.__TAURI__.window.getCurrentWindow().close();
	} catch (err) {
		$('error').textContent = String(err);
	} finally {
		$('go').disabled = false;
		$('go').textContent = words.pair_button;
	}
});

$('forget').addEventListener('click', async () => {
	if (!confirm(words.forget_sure)) return;
	await invoke('forget');
	await show();
});

show();
