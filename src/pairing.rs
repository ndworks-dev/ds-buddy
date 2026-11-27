//! The pairing flow: scan until a controller in pairing mode shows up, then
//! call it by address and retry until Windows completes the pairing. Runs on a
//! worker thread and reports progress through `emit`.
//!
//! Windows Settings gives up after one short scan and one attempt, which cheap
//! dongles (clone CSR8510s) rarely get right. Insisting is the whole trick.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use crate::{
    bt::{self, Address, Device, Search},
    i18n,
};

/// Each scan round lasts INQUIRY_UNITS x 1.28 s (~7.7 s).
const INQUIRY_UNITS: u8 = 6;
const RETRY_DELAY: Duration = Duration::from_secs(3);
/// Pairing mode turns itself off after a minute or two; remind the user this often.
const REMINDER_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Info,
    Good,
    Warn,
}

pub enum Event {
    /// What the flow is doing right now, for the big status line.
    Stage(Stage),
    Log(Kind, String),
}

#[derive(Clone, Debug)]
pub enum Stage {
    Searching { round: u32 },
    Pairing { name: String, attempt: u32 },
    Paired { name: String },
    TimedOut,
    Cancelled,
    Failed(String),
}

pub struct Options {
    /// Skip scanning and pair with this controller.
    pub address: Option<Address>,
    /// Make Windows forget the old pairing first (after the controller was used on a console).
    pub repair: bool,
    pub timeout: Duration,
}

pub fn run(opts: Options, cancel: &AtomicBool, mut emit: impl FnMut(Event)) {
    let stage = match flow(&opts, cancel, &mut emit) {
        Ok(stage) => stage,
        Err(e) => Stage::Failed(e.to_string()),
    };
    emit(Event::Stage(stage));
}

fn flow(opts: &Options, cancel: &AtomicBool, emit: &mut impl FnMut(Event)) -> Result<Stage, bt::Error> {
    let deadline = Instant::now() + opts.timeout;
    let known = known_devices()?;
    let mut name = i18n::pick("Controller", "Controle").to_string();

    if let Some(addr) = opts.address
        && let Some(d) = known.iter().find(|d| d.address == addr)
    {
        name = d.display_name().to_string();
    }

    if opts.repair {
        if let Some(addr) = opts.address
            && bt::remove(addr)?
        {
            emit(Event::Log(
                Kind::Info,
                tr!("Forgot the old pairing of {name} ({addr}).", "Pareamento antigo de {name} ({addr}) esquecido."),
            ));
        }
    } else if opts.address.is_none() {
        for d in known.iter().filter(|d| d.is_playstation_controller() && d.authenticated) {
            emit(Event::Log(
                Kind::Warn,
                tr!(
                    "Windows already has {} ({}) paired, so the search skips it even when it's on. If that's the one that won't connect, use Re-pair on the Controllers tab.",
                    "O Windows já tem {} ({}) pareado, então a busca vai ignorá-lo, mesmo ligado. Se é ele que não conecta, use Reparear na aba Controles.",
                    d.display_name(),
                    d.address
                ),
            ));
        }
    }

    let target = match opts.address {
        Some(addr) => addr,
        None => match find_controller(deadline, &known, cancel, emit)? {
            Found::Device(d) => {
                name = d.display_name().to_string();
                d.address
            }
            Found::TimedOut => return Ok(Stage::TimedOut),
            Found::Cancelled => return Ok(Stage::Cancelled),
        },
    };

    pair_with_retries(target, name, deadline, cancel, emit)
}

/// Windows' remembered devices, without scanning the air.
pub fn known_devices() -> Result<Vec<Device>, bt::Error> {
    bt::find_devices(Search { authenticated: true, remembered: true, connected: true, ..Default::default() })
}

enum Found {
    Device(Device),
    TimedOut,
    Cancelled,
}

fn find_controller(deadline: Instant, known: &[Device], cancel: &AtomicBool, emit: &mut impl FnMut(Event)) -> Result<Found, bt::Error> {
    let started = Instant::now();
    let mut last_reminder = Instant::now();
    let mut seen: Vec<Address> = Vec::new();
    let mut round = 0;

    while Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Found::Cancelled);
        }
        round += 1;
        emit(Event::Stage(Stage::Searching { round }));

        let found = bt::find_devices(Search { unknown: true, remembered: true, inquiry: Some(INQUIRY_UNITS), ..Default::default() })?;
        for d in &found {
            if !seen.contains(&d.address) {
                seen.push(d.address);
                emit(Event::Log(Kind::Info, tr!("Nearby: {} ({})", "Visto: {} ({})", d.display_name(), d.address)));
            }
        }
        // A remembered, not-yet-paired entry only counts if it is new: one that
        // was already there may be a stale leftover that is out of range.
        let controller = found.into_iter().find(|d| {
            d.is_playstation_controller() && !d.authenticated && (!d.remembered || !known.iter().any(|k| k.address == d.address))
        });
        if let Some(d) = controller {
            emit(Event::Log(
                Kind::Good,
                tr!(
                    "Found controller {} ({}) after {} s.",
                    "Controle encontrado: {} ({}) em {} s.",
                    d.display_name(),
                    d.address,
                    started.elapsed().as_secs()
                ),
            ));
            return Ok(Found::Device(d));
        }

        if last_reminder.elapsed() > REMINDER_EVERY {
            emit(Event::Log(
                Kind::Warn,
                tr!(
                    "Pairing mode turns itself off after a minute or two. If the light stopped flashing, hold PS for 10 s to turn the controller off, then hold Create + PS again.",
                    "O modo de pareamento desliga sozinho depois de um ou dois minutos. Se a luz parou de piscar, segure PS por 10 s e repita Create + PS."
                ),
            ));
            last_reminder = Instant::now();
        }
    }
    Ok(Found::TimedOut)
}

fn pair_with_retries(
    target: Address,
    name: String,
    deadline: Instant,
    cancel: &AtomicBool,
    emit: &mut impl FnMut(Event),
) -> Result<Stage, bt::Error> {
    // A half-finished pairing left over from Windows Settings blocks a fresh one.
    if bt::remove(target)? {
        emit(Event::Log(Kind::Info, tr!("Removed an unfinished pairing of {target}.", "Registro incompleto de {target} removido.")));
    }

    let mut attempt = 0;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Stage::Cancelled);
        }
        attempt += 1;
        emit(Event::Stage(Stage::Pairing { name: name.clone(), attempt }));

        match bt::pair(target) {
            Ok(()) => {
                emit(Event::Log(Kind::Good, tr!("Paired on attempt {attempt}.", "Pareado na tentativa {attempt}.")));
                return Ok(Stage::Paired { name });
            }
            Err(e) => emit(Event::Log(Kind::Warn, tr!("Attempt {attempt} failed ({e}).", "Tentativa {attempt} falhou ({e})."))),
        }
        thread::sleep(RETRY_DELAY);
    }
    Ok(Stage::TimedOut)
}
