// Opens the box's pages once the service worker answers this address: on the
// first run after installing, it may not be running yet.
async function go() {
	const reg = await navigator.serviceWorker.ready;
	if (!navigator.serviceWorker.controller) {
		reg.active?.postMessage({ t: 'claim' });
		await new Promise((ok) => setTimeout(ok, 300));
		location.reload();
		return;
	}
	location.replace('/');
}
void go();
