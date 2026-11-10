# Desktop app

Part of [ZOOMRIYA Connect](../README.md).

The desktop app is planned for Linux, Windows and macOS. It will use the
same iroh connection and Rust components as the phone apps (`keys`, `link`
and `punch`).

A browser extension cannot provide the connection by itself: browsers do not
let extensions open the UDP sockets or maintain the hole-punched connections
Connect needs. The desktop app provides that connection locally, then lets
the user's browser reach the box through it.

- **Your browser keeps using the box's address.** The app runs a local CONNECT
  proxy on `127.0.0.1`. A PAC file sends requests for box names
  (`*.zoomriya.app` and the family's own domain) through the app; other
  browsing goes directly to the internet. The browser establishes TLS with
  the box, so the address bar shows the box's real name and certificate. The
  desktop app cannot read that browser traffic.
- **Starts when you sign in.** The app will offer to start automatically:
  through a per-user startup entry on Windows, a login item on macOS, or a
  systemd user service (or XDG autostart) on Linux. It does not need
  administrator rights after installation. Automatic start is on by default,
  with a setting to turn it off.
- **Why it starts at sign-in, not at power-on.** The device key is kept in
  the user's own key store, which unlocks when they sign in. Starting before
  anyone signs in would require a machine-wide key, which offers weaker
  protection and is unnecessary for this feature.
- **Switching networks.** If the network changes, for example from Wi-Fi to a
  hotspot or after waking from sleep, iroh moves the connection and the app
  signals again. It retries with increasing pauses between attempts. At home,
  it connects directly without using the remote link.
- **Browser setup.** On Windows and macOS, the app configures the system PAC
  file, which Chrome and Edge follow. Firefox needs its own setting, or an
  "Open ZOOMRIYA in browser" button that launches a profile with the PAC
  configured. Linux support will start with a profile launcher; deeper
  desktop integration can follow.
- **Clear errors and cleanup.** If the app is not running, a local page
  explains why the box cannot be reached. The app removes its PAC setting
  when it quits or is uninstalled.
- **Pairing.** The box shows a code or link on its screen (laptops often have no camera), or a phone helps by scanning. The admin can revoke a laptop like a phone.
- **Key storage.** Windows will use TPM-backed storage; macOS will use
  Keychain, with Secure Enclave where available; Linux will use Secret
  Service or a TPM. This is not as strong as a phone's hardware keystore,
  especially on Linux, and the difference will be made clear in the docs.
- **Packaging.** Signed MSI or MSIX for Windows, a signed and notarized universal DMG for macOS, deb, rpm, AppImage or Flatpak for Linux. A tray icon shows the status, and updates are signed.
