//! The settings file, `headroom.toml`. A missing or unreadable file means the defaults.

use std::{fs, io, path::PathBuf};

use homeport::{Homeport, InstallKind};
use key_chord::KeyChord;
use serde::{Deserialize, Serialize};
use toml::{from_str, to_string};

const FILE_NAME: &str = "headroom.toml";

/// Written as `"none"` when the user has cleared it, so that clearing it survives a restart
/// instead of falling back to the default.
const NO_HOTKEY: &str = "none";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
	read_status_hotkey: String,
	/// Whether to look for a newer release each time Headroom starts.
	check_for_updates: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self { read_status_hotkey: default_hotkey().to_shortcut_string(), check_for_updates: true }
	}
}

impl Config {
	/// Reads the settings, or the defaults when there are none yet or they can't be read.
	pub fn load() -> Self {
		fs::read_to_string(home().config_dir().join(FILE_NAME))
			.ok()
			.and_then(|text| from_str(&text).ok())
			.unwrap_or_default()
	}

	pub fn save(&self) -> io::Result<()> {
		let text = to_string(self).map_err(io::Error::other)?;
		fs::write(path()?, text)
	}

	pub fn hotkey(&self) -> Option<KeyChord> {
		KeyChord::parse(&self.read_status_hotkey)
	}

	pub const fn check_for_updates(&self) -> bool {
		self.check_for_updates
	}

	pub fn set_hotkey(&mut self, chord: Option<&KeyChord>) {
		self.read_status_hotkey = chord.map_or_else(|| NO_HOTKEY.into(), KeyChord::to_shortcut_string);
	}
}

/// Alt+Shift+B reads the battery status from anywhere.
pub fn default_hotkey() -> KeyChord {
	KeyChord::new(false, true, true, "B")
}

/// Whether this copy was installed or keeps its settings beside it, which decides how an update
/// replaces it.
pub fn install_kind() -> InstallKind {
	home().kind()
}

fn home() -> Homeport {
	Homeport::new("Headroom").with_config_override("HEADROOM_CONFIG_DIR")
}

fn path() -> io::Result<PathBuf> {
	Ok(home().create_config_dir()?.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
	use toml::{from_str, to_string};

	use super::{Config, default_hotkey};

	#[test]
	fn defaults_to_alt_shift_b() {
		assert_eq!(Config::default().hotkey(), Some(default_hotkey()));
		assert_eq!(default_hotkey().to_shortcut_string(), "Alt+Shift+B");
	}

	#[test]
	fn a_cleared_hotkey_stays_cleared_after_a_round_trip() {
		let mut config = Config::default();
		config.set_hotkey(None);
		let reloaded: Config = from_str(&to_string(&config).unwrap()).unwrap();
		assert_eq!(reloaded.hotkey(), None);
	}

	#[test]
	fn a_missing_or_empty_file_gives_the_defaults() {
		assert_eq!(from_str::<Config>("").unwrap(), Config::default());
	}
}
