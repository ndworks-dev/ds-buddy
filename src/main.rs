//! DS Buddy: finds and pairs DualShock 4 / DualSense controllers over
//! Bluetooth on Windows, insisting until stubborn dongles get it right.
//! A native Slint window, software-rendered so it needs no GPU driver.
#![windows_subsystem = "windows"]

#[macro_use]
mod i18n;
mod bt;
mod pairing;
mod update;

use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use pairing::{Event, Kind, Options, Stage};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

slint::include_modules!();

const PAIR_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Keep the activity log short enough to read at a glance.
const MAX_LOG_LINES: usize = 60;
/// How often the controller list and adapter state are re-read, so pressing PS
/// shows up as "Conectado" without clicking anything.
const REFRESH_EVERY: Duration = Duration::from_secs(3);

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    i18n::apply_override();
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_log(ModelRc::from(log_model()));
    // Development aid for screenshots: open on a page (0 Pair, 1 Controllers).
    if let Some(page) = std::env::var("DS_BUDDY_PAGE").ok().and_then(|p| p.parse().ok()) {
        ui.set_page(page);
    }
    show_stage(&ui, None);
    refresh(&ui);

    let cancel = Arc::new(AtomicBool::new(false));

    {
        let weak = ui.as_weak();
        let cancel = cancel.clone();
        ui.on_start_pairing(move || {
            start(&weak, &cancel, Options { address: None, repair: false, timeout: PAIR_TIMEOUT });
        });
    }
    {
        let weak = ui.as_weak();
        let cancel = cancel.clone();
        ui.on_repair(move |address| match address.parse() {
            Ok(addr) => start(&weak, &cancel, Options { address: Some(addr), repair: true, timeout: PAIR_TIMEOUT }),
            Err(e) => toast(&weak, e),
        });
    }
    {
        let weak = ui.as_weak();
        let cancel = cancel.clone();
        ui.on_cancel(move || {
            cancel.store(true, Ordering::Relaxed);
            if let Some(ui) = weak.upgrade() {
                ui.set_cancelling(true);
                // Scans and pairing attempts can't be interrupted midway.
                ui.set_detail(
                    tr!(
                        "Finishing the current step. This can take a few seconds…",
                        "Terminando a etapa atual, isso pode levar alguns segundos…"
                    )
                    .into(),
                );
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_forget(move |address| {
            let message =
                match address.parse::<bt::Address>().map_err(|e| e.to_string()).and_then(|a| bt::remove(a).map_err(|e| e.to_string())) {
                    Ok(true) => tr!("Forgot {address}", "{address} esquecido"),
                    Ok(false) => tr!("Windows had already forgotten {address}", "O Windows já não conhecia {address}"),
                    Err(e) => tr!("Couldn't forget the controller: {e}", "Não deu para esquecer: {e}"),
                };
            toast(&weak, message);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_refresh(move || {
            if let Some(ui) = weak.upgrade() {
                refresh(&ui);
            }
        });
    }

    check_for_update(&ui);
    {
        let weak = ui.as_weak();
        ui.on_install_update(move || {
            let Some(ui) = weak.upgrade() else { return };
            match update::start(&ui.get_update_version()) {
                // The installer replaces this executable, so get out of its way.
                Ok(()) => {
                    let _ = slint::quit_event_loop();
                }
                Err(e) => toast(&weak, tr!("Couldn't start the update: {e}", "Não deu para iniciar a atualização: {e}")),
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_dismiss_update(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_update_version("".into());
            }
        });
    }

    let poll = slint::Timer::default();
    {
        let weak = ui.as_weak();
        poll.start(slint::TimerMode::Repeated, REFRESH_EVERY, move || {
            if let Some(ui) = weak.upgrade() {
                refresh(&ui);
            }
        });
    }

    // Slint has no restore event, so watch for the minimized -> shown
    // transition and force a full repaint (see `repaint` in app.slint).
    let restore = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let mut was_minimized = false;
        restore.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || {
            let Some(w) = weak.upgrade() else { return };
            let minimized = w.window().is_minimized();
            if was_minimized && !minimized {
                w.set_repaint(!w.get_repaint());
            }
            was_minimized = minimized;
        });
    }

    ui.run()
}

/// Asks GitHub for the latest release in the background and offers it when
/// it is newer. Offline or on any error there is simply no offer.
fn check_for_update(ui: &AppWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let Ok(latest) = update::latest_version() else { return };
        if !update::is_newer(&latest, env!("CARGO_PKG_VERSION")) {
            return;
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_update_version(latest.into());
            }
        });
    });
}

/// Starts the pairing flow on a worker thread; its events are applied on the UI thread.
fn start(weak: &Weak<AppWindow>, cancel: &Arc<AtomicBool>, opts: Options) {
    let Some(ui) = weak.upgrade() else { return };
    if ui.get_busy() {
        toast(weak, tr!("A pairing is already in progress", "Já tem um pareamento em andamento"));
        return;
    }
    if !bt::radio_available() {
        ui.set_adapter_ok(false);
        show_stage(
            &ui,
            Some(&Stage::Failed(tr!(
                "No Bluetooth adapter found. Plug in the dongle and turn Bluetooth on.",
                "Nenhum adaptador Bluetooth encontrado. Conecte o dongle e ligue o Bluetooth."
            ))),
        );
        return;
    }

    cancel.store(false, Ordering::Relaxed);
    ui.set_busy(true);
    ui.set_cancelling(false);
    log_model().set_vec(Vec::new());

    let weak = weak.clone();
    let cancel = cancel.clone();
    thread::spawn(move || {
        pairing::run(opts, &cancel, |event| {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    apply(&ui, event);
                }
            });
        });
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_busy(false);
                ui.set_cancelling(false);
                refresh(&ui);
            }
        });
    });
}

fn apply(ui: &AppWindow, event: Event) {
    match event {
        Event::Stage(stage) => show_stage(ui, Some(&stage)),
        Event::Log(kind, text) => {
            let model = log_model();
            if model.row_count() >= MAX_LOG_LINES {
                model.remove(0);
            }
            let kind = match kind {
                Kind::Info => 0,
                Kind::Good => 1,
                Kind::Warn => 2,
            };
            model.push(LogLine { time: now().into(), text: text.into(), kind });
        }
    }
}

/// Fills the headline card. `None` is the idle state before the first run.
fn show_stage(ui: &AppWindow, stage: Option<&Stage>) {
    let (phase, headline, detail): (i32, String, String) = match stage {
        None => (
            0,
            tr!("Ready to pair", "Pronto para parear"),
            tr!(
                "Put the controller in pairing mode and click Search and pair.",
                "Coloque o controle em modo de pareamento e clique em Procurar e parear."
            ),
        ),
        Some(Stage::Searching { round }) => (
            1,
            tr!("Searching for controllers…", "Procurando controles…"),
            tr!(
                "Scan {round}. Keep the controller flashing near the Bluetooth adapter.",
                "Rodada {round}. Deixe o controle piscando perto do adaptador Bluetooth."
            ),
        ),
        Some(Stage::Pairing { name, attempt, .. }) => (
            2,
            tr!("Pairing with {name}", "Pareando com {name}"),
            tr!(
                "Attempt {attempt}. If Windows asks you to confirm, allow it.",
                "Tentativa {attempt}. Se o Windows pedir confirmação, aceite."
            ),
        ),
        Some(Stage::Paired { name, .. }) => (
            3,
            tr!("{name} is paired!", "{name} pareado!"),
            tr!("From now on, just press the PS button to connect.", "Daqui pra frente, é só apertar o botão PS para conectar."),
        ),
        Some(Stage::TimedOut) => (
            4,
            tr!("Time's up", "Tempo esgotado"),
            tr!(
                "The controller wasn't found or didn't accept pairing within 10 minutes. Check that its light is still flashing and try again.",
                "O controle não foi encontrado ou não aceitou o pareamento em 10 minutos. Confira se a luz ainda pisca e tente de novo."
            ),
        ),
        Some(Stage::Cancelled) => (5, tr!("Canceled", "Cancelado"), tr!("The search was stopped.", "A busca foi interrompida.")),
        Some(Stage::Failed(e)) => (6, tr!("Something went wrong", "Algo deu errado"), e.clone()),
    };
    // Keep the "Cancelando…" note on screen until the flow actually stops.
    if ui.get_cancelling() && matches!(stage, Some(Stage::Searching { .. } | Stage::Pairing { .. })) {
        return;
    }
    ui.set_phase(phase);
    ui.set_headline(headline.into());
    ui.set_detail(detail.into());
}

/// Re-reads the adapter state and the paired controllers. Cheap: no radio scan.
fn refresh(ui: &AppWindow) {
    let adapter_ok = bt::radio_available();
    ui.set_adapter_ok(adapter_ok);
    let rows: Vec<ControllerRow> = if adapter_ok {
        pairing::known_devices()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.is_playstation_controller())
            .map(|d| ControllerRow {
                address: d.address.to_string().into(),
                name: d.display_name().into(),
                connected: d.connected,
                paired: d.authenticated,
            })
            .collect()
    } else {
        Vec::new()
    };
    // Only replace the model when something changed, so hovering a row doesn't flicker.
    let current = ui.get_controllers();
    if current.row_count() != rows.len() || current.iter().zip(&rows).any(|(a, b)| a != *b) {
        ui.set_controllers(ModelRc::from(Rc::new(VecModel::from(rows))));
    }
}

thread_local! {
    /// The activity log, shared with the window; lives on the UI thread.
    static LOG: Rc<VecModel<LogLine>> = Rc::new(VecModel::default());
}

fn log_model() -> Rc<VecModel<LogLine>> {
    LOG.with(Rc::clone)
}

fn toast(weak: &Weak<AppWindow>, message: String) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.set_toast(SharedString::from(message));
            let weak = w.as_weak();
            slint::Timer::single_shot(Duration::from_secs(3), move || {
                if let Some(w) = weak.upgrade() {
                    w.set_toast("".into());
                }
            });
        }
    });
}

fn now() -> String {
    // SAFETY: GetLocalTime only writes the SYSTEMTIME it is given.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}
