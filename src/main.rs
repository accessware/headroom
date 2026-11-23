#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod battery;
mod config;
#[cfg(windows)]
mod hid;
mod hotkey;
#[cfg(windows)]
mod speech;
mod update;

use std::{
	sync::{
		Arc, Mutex, PoisonError,
		mpsc::{self, RecvTimeoutError, Sender},
	},
	thread,
	time::Duration,
};

use doorbell::{Ring, ring};
use wxdragon::main as run_app;

use crate::app::{Channels, Latest};
#[cfg(windows)]
use crate::hid::read_battery;

#[cfg(not(windows))]
compile_error!("Headroom currently supports Windows only.");

const APP_NAME: &str = "Headroom";
/// Identifies the running copy to doorbell. Keep it stable across releases.
const INSTANCE_ID: &str = "headroom";
const POLL_INTERVAL: Duration = Duration::from_secs(5);

fn main() {
	// A second launch only asks the running copy to come to the front.
	let (raise_tx, raise) = mpsc::channel();
	match ring(INSTANCE_ID, &[] as &[&str]) {
		Ok(Ring::Delivered) => return,
		Ok(Ring::First(door)) => door.listen(move |_| {
			let _ = raise_tx.send(());
		}),
		// Not being able to check for another copy shouldn't stop this one.
		Err(_) => (),
	}
	let latest: Latest = Arc::new(Mutex::new("Headset: Reading…".into()));
	let stop_poller = spawn_poller(Arc::clone(&latest));
	let _ = run_app(move |_| app::run(Channels { latest, raise, stop_poller }));
}

/// Reads the headset on a background thread, so a slow or missing device never blocks the
/// window, and stores each reading in `latest`. Sending on, or dropping, the returned sender
/// stops the thread.
fn spawn_poller(latest: Latest) -> Sender<()> {
	let (stop_tx, stop_rx) = mpsc::channel::<()>();
	thread::spawn(move || {
		loop {
			let text = read_battery()
				.map_or_else(|error| format!("Headset: Unavailable — {error:#}"), |battery| battery.to_string());
			*latest.lock().unwrap_or_else(PoisonError::into_inner) = text;
			if !matches!(stop_rx.recv_timeout(POLL_INTERVAL), Err(RecvTimeoutError::Timeout)) {
				break;
			}
		}
	});
	stop_tx
}
