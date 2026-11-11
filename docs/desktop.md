# Desktop app

Part of [ZOOMRIYA Connect](../README.md).

The desktop app lets you use the box in your normal browser on a laptop, away
from home, at the same address you use at home. It's planned for Linux,
Windows and macOS, and shares its code with the phone app.

## Why an app, not a browser extension

Browsers don't let extensions make the kind of connection ZOOMRIYA Connect
needs. So a small app makes the connection, and your browser goes through it.

## How your browser reaches the box

The app runs a small helper on your laptop, called a proxy. A browser setting
(a PAC file) tells your browser: send the box's addresses (`*.zoomriya.app`,
or your own domain) through the app, and everything else to the internet as
usual.

Your browser still talks to the box itself and checks its certificate. The
address bar shows the box's real name. The app passes the traffic along, but
can't read it.

## Starting up

The app offers to start when you sign in to your laptop. It's on unless you
turn it off, and it doesn't need admin rights once it's installed.

- **Windows:** a startup entry for your account
- **macOS:** a login item
- **Linux:** a user service, or an autostart entry

It starts when you sign in, not when the laptop switches on, because its key
is kept in your own key store, which only unlocks when you sign in. Starting
earlier would mean one key for the whole laptop, which is less safe.

## Moving between networks

When you switch from Wi-Fi to a hotspot, or the laptop wakes up, the
connection moves over by itself. If it has to reconnect, it waits a little
longer between each try. At home, the app connects to the box directly over
your home network.

## Browser setup

- **Windows and macOS:** the app sets the system's PAC file. Chrome and Edge
  follow it.
- **Firefox:** needs its own setting. Or use the app's **Open ZOOMRIYA in
  browser** button, which opens a Firefox profile that's already set up.
- **Linux:** starts with the **Open ZOOMRIYA in browser** button. Better
  support can come later.

If the app isn't running, the browser shows a page on your laptop explaining
why the box can't be reached. The app removes its browser setting when it
quits or is uninstalled.

## Pairing a laptop

Laptops often have no camera, so the box shows a code or link on its screen
instead of a QR code. Or a paired phone can help. An admin removes a laptop
the same way as a phone.

## What locks the key

As on a phone, the app's key stays on the laptop, locked by the system's own
key store.

| System | Locked by |
|---|---|
| Windows | The TPM security chip |
| macOS | Keychain, with the Secure Enclave on Macs that have one |
| Linux | Secret Service, or the TPM |

This isn't as strong as a phone's security chip, especially on Linux. We'll
say so plainly.

## Installers

| System | Installer |
|---|---|
| Windows | Signed MSI or MSIX |
| macOS | Signed and notarised DMG, for Intel and Apple chips |
| Linux | deb, rpm, AppImage or Flatpak |

A tray icon shows whether you're connected. Updates are signed.
