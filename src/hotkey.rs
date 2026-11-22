//! The system-wide hotkey that reads the battery status, and the keymap the shortcut dialog edits.

use key_chord::KeyChord;
use wx_utils::shortcuts::{ShortcutModel, TabScope};

use crate::config::default_hotkey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
	ReadStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
	pub read_status: Option<KeyChord>,
}

impl ShortcutModel for Keymap {
	type Action = Action;

	fn tabs(&self) -> Vec<String> {
		vec!["System-wide".into()]
	}

	fn tab_scope(&self, _tab: usize) -> TabScope {
		TabScope::Global
	}

	fn actions(&self, _tab: usize) -> Vec<Action> {
		vec![Action::ReadStatus]
	}

	fn action_name(&self, action: Action) -> String {
		match action {
			Action::ReadStatus => "Read the battery status".into(),
		}
	}

	fn chord(&self, _tab: usize, action: Action) -> Option<KeyChord> {
		match action {
			Action::ReadStatus => self.read_status.clone(),
		}
	}

	fn set_chord(&mut self, _tab: usize, action: Action, chord: Option<KeyChord>) {
		match action {
			Action::ReadStatus => self.read_status = chord,
		}
	}

	fn reset_action(&mut self, _tab: usize, action: Action) {
		match action {
			Action::ReadStatus => self.read_status = Some(default_hotkey()),
		}
	}

	fn reset_all(&mut self, tab: usize) {
		self.reset_action(tab, Action::ReadStatus);
	}
}

/// The hotkey as a sentence, for the window and the tray tooltip.
pub fn describe(chord: Option<&KeyChord>) -> String {
	chord.map_or_else(
		|| "No hotkey is set for reading the battery status.".into(),
		|chord| format!("Press {} anywhere to hear the battery status.", chord.to_display_string()),
	)
}
