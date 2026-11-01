# Desktop app

Part of [ZOOMRIYA Connect](../README.md).

A native app for Linux, Windows and macOS, with the same iroh endpoint and Rust crates as the phone link (`keys`, `link`, `punch`). A browser extension can't do this job: extensions can't open UDP sockets or hold hole-punched connections.

- **Browsers see the box address.** The app runs a local CONNECT proxy on `127.0.0.1`. A PAC file sends only box names (`*.zoomriya.app` and the family's own domain) through it; all other browsing goes direct. The browser does TLS with the box itself, so the address bar shows the real name and a valid certificate, and the app can't read the traffic.
- **Starts by itself.** It starts when the user signs in: a per-user startup entry on Windows, a login item on macOS, a systemd user service (or XDG autostart) on Linux. No admin rights after install. One question at install, on by default, with a switch in settings.
- **Why sign-in, not power-on.** The device key lives in the user's own key store, which unlocks at sign-in. Starting before anyone signs in would need a machine-level key, which is weaker, and this feature doesn't need it.
- **Roaming.** iroh moves the connection, and the app signals again, when the network changes (Wi-Fi to hotspot, after sleep); it retries with backoff. At home it connects directly and skips the link.
- **Browser setup.** The app sets the system PAC on Windows and macOS (Chrome and Edge follow it). Firefox needs its own setting, or an "Open ZOOMRIYA in browser" button that starts a profile with the PAC already set. On Linux the profile launcher comes first, with desktop integration later.
- **Fails clearly.** If the app isn't running, a local page explains it. The app removes its PAC setting when it quits or is uninstalled.
- **Pairing.** The box shows a code or link on its screen (laptops often have no camera), or a phone helps by scanning. The admin can revoke a laptop like a phone.
- **Key storage.** TPM-backed storage on Windows, Keychain on macOS (Secure Enclave where available), Secret Service or TPM on Linux. This is weaker than a phone's hardware keystore, especially on Linux, and the docs say so.
- **Packaging.** Signed MSI or MSIX for Windows, a signed and notarized universal DMG for macOS, deb, rpm, AppImage or Flatpak for Linux. A tray icon shows the status, and updates are signed.
