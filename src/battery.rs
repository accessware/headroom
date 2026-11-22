//! Status report decoding, independent of the Windows HID transport.

use std::fmt::{self, Display, Formatter};

use anyhow::{Result, ensure};

/// The report id and command byte that open both the status query and its reply.
pub const STATUS_HEADER: [u8; 2] = [0x06, 0xb0];

/// Bytes a reply needs before every field below is present.
const MIN_REPORT_LEN: usize = 16;

/// The hardware reports nine levels (0–8), not a continuous percentage.
const FULL_STEP: u8 = 8;

#[derive(Debug, PartialEq, Eq)]
pub struct Battery {
	headset_step: u8,
	dock_step: u8,
	headset_state: u8,
}

impl Battery {
	pub fn from_report(report: &[u8]) -> Result<Self> {
		ensure!(report.len() >= MIN_REPORT_LEN, "Base station returned an incomplete status report");
		ensure!(report[..2] == STATUS_HEADER, "Unexpected base station report");
		let (headset_step, dock_step, headset_state) = (report[6], report[7], report[15]);
		ensure!(
			headset_step <= FULL_STEP && dock_step <= FULL_STEP,
			"Base station returned an unsupported battery level"
		);
		Ok(Self { headset_step, dock_step, headset_state })
	}

	pub fn headset(&self) -> String {
		match self.headset_state {
			0x01 => "Off or out of range".into(),
			0x02 => format!("{} — Charging", percentage(self.headset_step)),
			0x08 => format!("{} — On battery", percentage(self.headset_step)),
			state => format!("Unavailable — unknown connection state (0x{state:02x})"),
		}
	}

	/// As in GG, a zero spare reading is treated as an empty charging slot, and the charging
	/// state is inferred from the level: the headset's state byte doesn't apply to the spare.
	pub fn dock(&self) -> String {
		match self.dock_step {
			0 => "No battery detected".into(),
			FULL_STEP => "100% — Charged".into(),
			step => format!("{} — Charging", percentage(step)),
		}
	}
}

impl Display for Battery {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "Headset: {}\nDock: {}", self.headset(), self.dock())
	}
}

fn percentage(step: u8) -> String {
	if step == FULL_STEP { "100%".into() } else { format!("About {}%", u16::from(step) * 100 / u16::from(FULL_STEP)) }
}

#[cfg(test)]
mod tests {
	use super::{Battery, STATUS_HEADER};

	fn report(headset: u8, dock: u8, state: u8) -> [u8; 64] {
		let mut bytes = [0; 64];
		bytes[..2].copy_from_slice(&STATUS_HEADER);
		bytes[6] = headset;
		bytes[7] = dock;
		bytes[15] = state;
		bytes
	}

	#[test]
	fn decodes_the_live_status_report() {
		let bytes = [0x06, 0xb0, 0, 0, 1, 0, 3, 8, 8, 1, 2, 10, 5, 0, 8, 8];
		assert_eq!(
			Battery::from_report(&bytes).unwrap().to_string(),
			"Headset: About 37% — On battery\nDock: 100% — Charged"
		);
	}

	#[test]
	fn disconnected_headset_does_not_hide_dock_or_show_stale_headset_level() {
		let battery = Battery::from_report(&report(7, 4, 1)).unwrap();
		assert_eq!(battery.headset(), "Off or out of range");
		assert_eq!(battery.dock(), "About 50% — Charging");
	}

	#[test]
	fn charging_and_empty_dock_are_separate() {
		let battery = Battery::from_report(&report(4, 0, 2)).unwrap();
		assert_eq!(battery.headset(), "About 50% — Charging");
		assert_eq!(battery.dock(), "No battery detected");
	}

	#[test]
	fn validates_every_battery_step() {
		for step in 0..=8 {
			assert!(Battery::from_report(&report(step, step, 8)).is_ok());
		}
		for step in 9..=u8::MAX {
			assert!(Battery::from_report(&report(step, 8, 8)).is_err());
			assert!(Battery::from_report(&report(8, step, 8)).is_err());
		}
	}

	#[test]
	fn rejects_truncated_and_unrelated_reports() {
		for length in 0..16 {
			assert!(Battery::from_report(&report(8, 8, 8)[..length]).is_err());
		}
		let mut bytes = report(8, 8, 8);
		bytes[0] = 7;
		assert!(Battery::from_report(&bytes).is_err());
		bytes[0] = 6;
		bytes[1] = 0x20;
		assert!(Battery::from_report(&bytes).is_err());
	}

	#[test]
	fn unknown_state_is_not_presented_as_a_connected_headset() {
		assert!(Battery::from_report(&report(8, 8, 0xff)).unwrap().headset().contains("unknown connection state"));
	}
}
