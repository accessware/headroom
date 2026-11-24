//! Supported headsets and their status report decoding, independent of the Windows HID transport.

use std::fmt::{self, Display, Formatter};

use anyhow::{Result, ensure};

/// How to find, query, and decode one headset family. Reports are handled exactly as Windows
/// delivers them, so the first byte is the HID report id (zero when the device doesn't use ids).
pub struct Profile {
	pub vendor_id: u16,
	pub product_ids: &'static [u16],
	/// The USB interface that carries the status collection, as it appears in a device path.
	pub interface: &'static str,
	pub usage_page: u16,
	/// The size of every input and output report, report id included.
	pub report_len: usize,
	/// Opens both the status query and its reply: the report id and the command byte.
	pub header: [u8; 2],
	/// Bytes a reply needs before every field its decoder reads is present.
	min_len: usize,
	decode: fn(&[u8]) -> Result<Battery>,
}

impl Profile {
	pub fn decode(&self, report: &[u8]) -> Result<Battery> {
		ensure!(report.len() >= self.min_len, "Headset returned an incomplete status report");
		ensure!(report.starts_with(&self.header), "Unexpected headset report");
		(self.decode)(report)
	}
}

/// The Nova Pro Wireless base station reports the headset and a spare battery in its dock, each
/// in nine levels (0–8).
pub static NOVA_PRO_WIRELESS: Profile = Profile {
	vendor_id: 0x1038,
	product_ids: &[0x12e0, 0x12e5],
	interface: "mi_04",
	usage_page: 0xffc0,
	report_len: 64,
	header: [0x06, 0xb0],
	min_len: 16,
	decode: decode_nova_pro,
};

/// The Nova 7X dongle reports only the headset, in five levels (0–4) on the product ids listed.
pub static NOVA_7X: Profile = Profile {
	vendor_id: 0x1038,
	product_ids: &[0x2206],
	interface: "mi_03",
	usage_page: 0xffc0,
	report_len: 65,
	header: [0x00, 0xb0],
	min_len: 5,
	decode: decode_nova_7x,
};

/// The Nova Elite base station reports the headset and a spare battery in its charger, as exact
/// percentages, in the same report layout as the Nova Pro's.
pub static NOVA_ELITE: Profile = Profile {
	vendor_id: 0x1038,
	product_ids: &[0x2244, 0x2270],
	interface: "mi_03",
	usage_page: 0xffc0,
	report_len: 64,
	header: [0x01, 0xb0],
	min_len: 16,
	decode: decode_nova_elite,
};

pub static PROFILES: [&Profile; 3] = [&NOVA_PRO_WIRELESS, &NOVA_7X, &NOVA_ELITE];

/// The Nova Pro's battery scale, for the headset and its dock.
const NOVA_PRO_FULL_STEP: u8 = 8;

#[derive(Debug, PartialEq, Eq)]
enum State {
	Offline,
	Charging,
	OnBattery,
	Unknown(u8),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Battery {
	headset_percent: u8,
	headset_state: State,
	/// The spare battery's level, for headsets that have a dock.
	dock_percent: Option<u8>,
	/// The levels are exact percentages, rather than converted from a coarse scale of steps.
	exact: bool,
}

/// A level on a scale of `0..=full` steps as a percentage, rounded down.
fn step_percent(step: u8, full: u8) -> u8 {
	u8::try_from(u16::from(step) * 100 / u16::from(full)).unwrap_or(100)
}

/// The headset state byte shared by the Nova Pro and Nova Elite base stations.
const fn base_station_state(state: u8) -> State {
	match state {
		0x01 => State::Offline,
		0x02 => State::Charging,
		0x08 => State::OnBattery,
		state => State::Unknown(state),
	}
}

fn decode_nova_pro(report: &[u8]) -> Result<Battery> {
	let (headset_step, dock_step, state) = (report[6], report[7], report[15]);
	ensure!(
		headset_step <= NOVA_PRO_FULL_STEP && dock_step <= NOVA_PRO_FULL_STEP,
		"Base station returned an unsupported battery level"
	);
	Ok(Battery {
		headset_percent: step_percent(headset_step, NOVA_PRO_FULL_STEP),
		headset_state: base_station_state(state),
		dock_percent: Some(step_percent(dock_step, NOVA_PRO_FULL_STEP)),
		exact: false,
	})
}

fn decode_nova_elite(report: &[u8]) -> Result<Battery> {
	let (headset_percent, dock_percent, state) = (report[6], report[7], report[15]);
	ensure!(headset_percent <= 100 && dock_percent <= 100, "Base station returned an unsupported battery level");
	Ok(Battery {
		headset_percent,
		headset_state: base_station_state(state),
		dock_percent: Some(dock_percent),
		exact: true,
	})
}

fn decode_nova_7x(report: &[u8]) -> Result<Battery> {
	const FULL_STEP: u8 = 4;
	let (step, state) = (report[3], report[4]);
	let headset_state = match state {
		0x00 => State::Offline,
		0x01 | 0x02 => State::Charging,
		0x03 => State::OnBattery,
		state => State::Unknown(state),
	};
	// An offline headset's level is stale, so it isn't checked or shown.
	if headset_state != State::Offline {
		ensure!(step <= FULL_STEP, "Headset returned an unsupported battery level");
	}
	Ok(Battery {
		headset_percent: step_percent(step.min(FULL_STEP), FULL_STEP),
		headset_state,
		dock_percent: None,
		exact: false,
	})
}

impl Battery {
	pub fn headset(&self) -> String {
		match self.headset_state {
			State::Offline => "Off or out of range".into(),
			State::Charging => format!("{} — Charging", self.percentage(self.headset_percent)),
			State::OnBattery => format!("{} — On battery", self.percentage(self.headset_percent)),
			State::Unknown(state) => format!("Unavailable — unknown connection state (0x{state:02x})"),
		}
	}

	/// As in GG, a zero spare reading is treated as an empty charging slot, and the charging
	/// state is inferred from the level: the headset's state byte doesn't apply to the spare.
	pub fn dock(&self) -> Option<String> {
		self.dock_percent.map(|percent| match percent {
			0 => "No battery detected".into(),
			100 => "100% — Charged".into(),
			percent => format!("{} — Charging", self.percentage(percent)),
		})
	}

	/// A level converted from steps is labeled "About", except the highest step.
	fn percentage(&self, percent: u8) -> String {
		if self.exact || percent == 100 { format!("{percent}%") } else { format!("About {percent}%") }
	}
}

impl Display for Battery {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "Headset: {}", self.headset())?;
		if let Some(dock) = self.dock() {
			write!(f, "\nDock: {dock}")?;
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::{NOVA_7X, NOVA_ELITE, NOVA_PRO_WIRELESS};

	fn nova_pro(headset: u8, dock: u8, state: u8) -> [u8; 64] {
		let mut bytes = [0; 64];
		bytes[..2].copy_from_slice(&NOVA_PRO_WIRELESS.header);
		bytes[6] = headset;
		bytes[7] = dock;
		bytes[15] = state;
		bytes
	}

	fn nova_7x(level: u8, state: u8) -> [u8; 65] {
		let mut bytes = [0; 65];
		bytes[..2].copy_from_slice(&NOVA_7X.header);
		bytes[3] = level;
		bytes[4] = state;
		bytes
	}

	#[test]
	fn decodes_the_live_nova_pro_report() {
		let bytes = [0x06, 0xb0, 0, 0, 1, 0, 3, 8, 8, 1, 2, 10, 5, 0, 8, 8];
		assert_eq!(
			NOVA_PRO_WIRELESS.decode(&bytes).unwrap().to_string(),
			"Headset: About 37% — On battery\nDock: 100% — Charged"
		);
	}

	#[test]
	fn disconnected_headset_does_not_hide_dock_or_show_stale_headset_level() {
		let battery = NOVA_PRO_WIRELESS.decode(&nova_pro(7, 4, 1)).unwrap();
		assert_eq!(battery.headset(), "Off or out of range");
		assert_eq!(battery.dock().unwrap(), "About 50% — Charging");
	}

	#[test]
	fn charging_and_empty_dock_are_separate() {
		let battery = NOVA_PRO_WIRELESS.decode(&nova_pro(4, 0, 2)).unwrap();
		assert_eq!(battery.headset(), "About 50% — Charging");
		assert_eq!(battery.dock().unwrap(), "No battery detected");
	}

	#[test]
	fn validates_every_nova_pro_battery_step() {
		for step in 0..=8 {
			assert!(NOVA_PRO_WIRELESS.decode(&nova_pro(step, step, 8)).is_ok());
		}
		for step in 9..=u8::MAX {
			assert!(NOVA_PRO_WIRELESS.decode(&nova_pro(step, 8, 8)).is_err());
			assert!(NOVA_PRO_WIRELESS.decode(&nova_pro(8, step, 8)).is_err());
		}
	}

	#[test]
	fn labels_every_nova_pro_step_as_approximate_except_the_highest() {
		for (step, headset, dock) in [
			(0, "About 0% — On battery", "No battery detected"),
			(1, "About 12% — On battery", "About 12% — Charging"),
			(2, "About 25% — On battery", "About 25% — Charging"),
			(3, "About 37% — On battery", "About 37% — Charging"),
			(4, "About 50% — On battery", "About 50% — Charging"),
			(5, "About 62% — On battery", "About 62% — Charging"),
			(6, "About 75% — On battery", "About 75% — Charging"),
			(7, "About 87% — On battery", "About 87% — Charging"),
			(8, "100% — On battery", "100% — Charged"),
		] {
			let battery = NOVA_PRO_WIRELESS.decode(&nova_pro(step, step, 8)).unwrap();
			assert_eq!(battery.headset(), headset);
			assert_eq!(battery.dock().unwrap(), dock);
		}
	}

	#[test]
	fn rejects_truncated_and_unrelated_nova_pro_reports() {
		for length in 0..16 {
			assert!(NOVA_PRO_WIRELESS.decode(&nova_pro(8, 8, 8)[..length]).is_err());
		}
		let mut bytes = nova_pro(8, 8, 8);
		bytes[0] = 7;
		assert!(NOVA_PRO_WIRELESS.decode(&bytes).is_err());
		bytes[0] = 6;
		bytes[1] = 0x20;
		assert!(NOVA_PRO_WIRELESS.decode(&bytes).is_err());
	}

	#[test]
	fn unknown_nova_pro_state_is_not_presented_as_a_connected_headset() {
		assert!(
			NOVA_PRO_WIRELESS.decode(&nova_pro(8, 8, 0xff)).unwrap().headset().contains("unknown connection state")
		);
	}

	#[test]
	fn decodes_the_live_nova_7x_report() {
		// A reply from a Nova 7X (1038:2206) at 75%, not charging, with a leading report id of zero.
		let mut bytes = [0; 65];
		bytes[..16]
			.copy_from_slice(&[0x00, 0xb0, 0x03, 0x03, 0x03, 0x64, 0x64, 0, 0, 0, 0x2e, 0x30, 0x30, 0x30, 0x30, 0x30]);
		assert_eq!(NOVA_7X.decode(&bytes).unwrap().to_string(), "Headset: About 75% — On battery");
	}

	#[test]
	fn nova_7x_has_no_dock() {
		assert_eq!(NOVA_7X.decode(&nova_7x(4, 3)).unwrap().dock(), None);
	}

	#[test]
	fn maps_every_nova_7x_level() {
		for (level, text) in [
			(0, "About 0% — On battery"),
			(1, "About 25% — On battery"),
			(2, "About 50% — On battery"),
			(3, "About 75% — On battery"),
			(4, "100% — On battery"),
		] {
			assert_eq!(NOVA_7X.decode(&nova_7x(level, 3)).unwrap().headset(), text);
		}
		assert!(NOVA_7X.decode(&nova_7x(5, 3)).is_err());
		assert!(NOVA_7X.decode(&nova_7x(0xff, 3)).is_err());
	}

	#[test]
	fn nova_7x_states() {
		assert_eq!(NOVA_7X.decode(&nova_7x(0xff, 0)).unwrap().headset(), "Off or out of range");
		assert_eq!(NOVA_7X.decode(&nova_7x(2, 1)).unwrap().headset(), "About 50% — Charging");
		assert_eq!(NOVA_7X.decode(&nova_7x(2, 2)).unwrap().headset(), "About 50% — Charging");
		assert!(NOVA_7X.decode(&nova_7x(4, 9)).unwrap().headset().contains("unknown connection state (0x09)"));
	}

	#[test]
	fn rejects_truncated_and_unrelated_nova_7x_reports() {
		for length in 0..5 {
			assert!(NOVA_7X.decode(&nova_7x(4, 3)[..length]).is_err());
		}
		let mut bytes = nova_7x(4, 3);
		bytes[0] = 1;
		assert!(NOVA_7X.decode(&bytes).is_err());
		bytes[0] = 0;
		bytes[1] = 0x20;
		assert!(NOVA_7X.decode(&bytes).is_err());
	}

	fn nova_elite(headset: u8, dock: u8, state: u8) -> [u8; 64] {
		let mut bytes = [0; 64];
		bytes[..2].copy_from_slice(&NOVA_ELITE.header);
		bytes[6] = headset;
		bytes[7] = dock;
		bytes[15] = state;
		bytes
	}

	#[test]
	fn decodes_the_live_nova_elite_reports() {
		// Replies from a Nova Elite (1038:2244), captured with the headset on and then off.
		let on = [0x01, 0xb0, 0, 0, 1, 0, 0x59, 0x64, 8, 0, 0, 0x0a, 0, 0, 8, 8, 3];
		assert_eq!(NOVA_ELITE.decode(&on).unwrap().to_string(), "Headset: 89% — On battery\nDock: 100% — Charged");
		let charging_spare = [0x01, 0xb0, 0, 0, 1, 0, 0x64, 0x57, 8, 0, 0, 0x0a, 0, 0, 8, 8, 3];
		assert_eq!(
			NOVA_ELITE.decode(&charging_spare).unwrap().to_string(),
			"Headset: 100% — On battery\nDock: 87% — Charging"
		);
		let off = [0x01, 0xb0, 0, 0, 1, 0, 0, 0x64, 8, 0, 0, 0x0a, 0, 0, 4, 1, 3];
		assert_eq!(NOVA_ELITE.decode(&off).unwrap().to_string(), "Headset: Off or out of range\nDock: 100% — Charged");
	}

	#[test]
	fn nova_elite_levels_are_exact() {
		for percent in [1, 12, 37, 50, 99] {
			let battery = NOVA_ELITE.decode(&nova_elite(percent, percent, 8)).unwrap();
			assert_eq!(battery.headset(), format!("{percent}% — On battery"));
			assert_eq!(battery.dock().unwrap(), format!("{percent}% — Charging"));
		}
		assert_eq!(NOVA_ELITE.decode(&nova_elite(0, 0, 8)).unwrap().dock().unwrap(), "No battery detected");
	}

	#[test]
	fn validates_nova_elite_percentages() {
		for percent in 0..=100 {
			assert!(NOVA_ELITE.decode(&nova_elite(percent, percent, 8)).is_ok());
		}
		for percent in 101..=u8::MAX {
			assert!(NOVA_ELITE.decode(&nova_elite(percent, 100, 8)).is_err());
			assert!(NOVA_ELITE.decode(&nova_elite(100, percent, 8)).is_err());
		}
	}

	#[test]
	fn rejects_truncated_and_unrelated_nova_elite_reports() {
		for length in 0..16 {
			assert!(NOVA_ELITE.decode(&nova_elite(100, 100, 8)[..length]).is_err());
		}
		// The Nova Pro's report id, and the base station's event reports, aren't status replies.
		for header in [[0x06, 0xb0], [0x07, 0xb7], [0x01, 0x10]] {
			let mut bytes = nova_elite(100, 100, 8);
			bytes[..2].copy_from_slice(&header);
			assert!(NOVA_ELITE.decode(&bytes).is_err());
		}
	}

	#[test]
	fn unknown_nova_elite_state_is_not_presented_as_a_connected_headset() {
		assert!(NOVA_ELITE.decode(&nova_elite(100, 100, 0xff)).unwrap().headset().contains("unknown connection state"));
	}
}
