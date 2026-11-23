//! The ZOOMRIYA laptop app: in the menu bar or system tray, it only connects
//! the laptop to the box. Everything else happens in the laptop's own
//! browser, at `zoomriya.localhost:7480` (the apps) and
//! `zoomriyabox.localhost:7480` (Box Desktop) (docs/direct-plus.md, §10).
//!
//! | Menu | What |
//! |---|---|
//! | Connected to your box / Looking for your box… | How it is now |
//! | Open ZOOMRIYA | The apps, in the default browser |
//! | Open Box Desktop | Box Desktop, in the default browser |
//! | Pair with a box… | The only window: the short code, or forgetting the box |
//! | Quit ZOOMRIYA | |

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod words;

use std::{sync::Arc, time::Duration};

use serde::Serialize;
use tauri::{
    AppHandle, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, Wry,
    async_runtime::{self, JoinHandle},
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tracing::warn;
use words::Words;
use zoomriya_laptop::{
    Client, PairError, ToTheBox,
    serve::{PORT, Server, Site},
    store::{Store, default_dir},
};

/// The browser server while the laptop is paired.
struct Running {
    client: Arc<Client>,
    server: Arc<Server<ToTheBox>>,
    task: JoinHandle<()>,
}

/// What the app keeps.
struct App {
    store: Store,
    words: &'static Words,
    running: tokio::sync::Mutex<Option<Running>>,
    status: MenuItem<Wry>,
}

impl App {
    fn say(&self, text: &str) {
        let _ = self.status.set_text(text);
    }

    /// Starts serving the paired box to the browser.
    async fn start(&self) -> anyhow::Result<()> {
        let mut slot = self.running.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        let (client, server) = zoomriya_laptop::server(&self.store, PORT).await?;
        let listeners = match server.bind().await {
            Ok(l) => l,
            Err(e) => {
                client.close().await;
                return Err(e);
            }
        };
        let task = async_runtime::spawn(server.clone().serve(listeners));
        *slot = Some(Running {
            client,
            server,
            task,
        });
        Ok(())
    }

    async fn stop(&self) {
        if let Some(r) = self.running.lock().await.take() {
            r.task.abort();
            r.client.close().await;
        }
    }

    /// A launch link for `site`, if the laptop is paired.
    async fn launch(&self, site: Site) -> Option<String> {
        let slot = self.running.lock().await;
        slot.as_ref()
            .and_then(|r| r.server.launch_url(site, "/").ok())
    }

    /// Reaches the box now, and says how it went.
    async fn check(&self) {
        let client = self.running.lock().await.as_ref().map(|r| r.client.clone());
        match client {
            None => self.say(self.words.not_paired),
            Some(c) => {
                let up = c.connection().await.is_ok();
                self.say(if up {
                    self.words.connected
                } else {
                    self.words.looking
                });
            }
        }
    }
}

fn show_pair_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("pair") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let words = app.state::<Arc<App>>().words;
    if let Err(e) = WebviewWindowBuilder::new(app, "pair", WebviewUrl::App("index.html".into()))
        .title(words.title)
        .inner_size(440.0, 340.0)
        .resizable(false)
        .center()
        .build()
    {
        warn!("pairing window: {e}");
    }
}

fn open(app: &AppHandle, site: Site) {
    let a = app.state::<Arc<App>>().inner().clone();
    let handle = app.clone();
    async_runtime::spawn(async move {
        match a.launch(site).await {
            Some(url) => {
                if let Err(e) = zoomriya_laptop::open_in_browser(&url) {
                    warn!("{e:#}");
                }
            }
            None => show_pair_window(&handle),
        }
    });
}

#[derive(Serialize)]
struct Info {
    paired: bool,
    words: &'static Words,
}

/// What the pairing window shows.
#[tauri::command]
async fn info(app: State<'_, Arc<App>>) -> Result<Info, String> {
    Ok(Info {
        paired: app.store.paired().ok().flatten().is_some(),
        words: app.words,
    })
}

/// Pairs with the box showing `code`, then opens ZOOMRIYA.
#[tauri::command]
async fn pair(app: State<'_, Arc<App>>, code: String) -> Result<(), String> {
    let w = app.words;
    zoomriya_laptop::pair_at_home(&app.store, &code)
        .await
        .map_err(|e| {
            match e {
                PairError::NotHome => w.not_home,
                PairError::WrongCode => w.wrong_code,
                PairError::Unreachable => w.unreachable,
                PairError::Refused(_) | PairError::Broken => w.broken,
            }
            .to_string()
        })?;
    app.start().await.map_err(|_| w.broken.to_string())?;
    app.check().await;
    if let Some(url) = app.launch(Site::Apps).await {
        let _ = zoomriya_laptop::open_in_browser(&url);
    }
    Ok(())
}

/// Forgets the box: this laptop stops reaching it.
#[tauri::command]
async fn forget(app: State<'_, Arc<App>>) -> Result<(), String> {
    app.stop().await;
    app.store.forget().map_err(|e| e.to_string())?;
    app.say(app.words.not_paired);
    Ok(())
}

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let words = words::for_this_laptop();

    let app = tauri::Builder::default()
        // Opening the app again opens ZOOMRIYA from the copy already running.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            open(app, Site::Apps)
        }))
        .invoke_handler(tauri::generate_handler![info, pair, forget])
        .setup(move |app| {
            let dir = default_dir()?;
            let status = MenuItem::with_id(app, "status", words.looking, false, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &status,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "open", words.open, true, None::<&str>)?,
                    &MenuItem::with_id(app, "desktop", words.open_desktop, true, None::<&str>)?,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "pair", words.pair, true, None::<&str>)?,
                    &MenuItem::with_id(app, "quit", words.quit, true, None::<&str>)?,
                ],
            )?;
            let mut tray = TrayIconBuilder::with_id("zoomriya")
                .tooltip("ZOOMRIYA")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => open(app, Site::Apps),
                    "desktop" => open(app, Site::Desktop),
                    "pair" => show_pair_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let state = Arc::new(App {
                store: Store::new(dir),
                words,
                running: tokio::sync::Mutex::new(None),
                status,
            });
            app.manage(state.clone());
            let handle = app.handle().clone();
            async_runtime::spawn(async move {
                let paired = state.store.paired().ok().flatten().is_some();
                if !paired {
                    state.say(words.not_paired);
                    show_pair_window(&handle);
                } else if let Err(e) = state.start().await {
                    warn!("{e:#}");
                    state.say(words.already_running);
                    return;
                }
                // Keep the connection up, so pages open at once.
                loop {
                    state.check().await;
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ZOOMRIYA could not start: {e}");
            std::process::exit(1);
        }
    };
    app.run(|_, event| {
        // Closing the pairing window keeps the app in the tray; Quit ends it.
        if let RunEvent::ExitRequested { api, code, .. } = event
            && code.is_none()
        {
            api.prevent_exit();
        }
    });
}
