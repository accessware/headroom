//! Checking GitHub for a newer Headroom, and installing it, through ship-shape.

use std::sync::Arc;

use ship_shape::{
	UpdateChannel, UpdaterConfig,
	ui::{self, CheckTrigger},
};
use wxdragon::prelude::*;

use crate::{APP_NAME, config};

const REPOSITORY: &str = "accessware/headroom";
/// The minisign key Headroom's releases are signed with, the same one Gitview's are. An update
/// whose signature it doesn't verify is never installed.
const PUBLIC_KEY: &str = "RWSPPY7CuiOcORz9tJEWN3J9/Q1pbDZ03F01ozk+tc4Thfdiv5EQpa1p";

/// Looks for a newer release. An automatic check only speaks up when there's one; a check the
/// user asked for also says when Headroom is up to date, or that it couldn't tell. Once an update
/// is ready, ship-shape closes `frame` the normal way, which ends Headroom, and installs it.
pub fn check(frame: &Frame, automatic: bool) {
	let updater = Arc::new(
		UpdaterConfig::new(REPOSITORY, "headroom", APP_NAME, PUBLIC_KEY, env!("CARGO_PKG_VERSION"))
			.with_install_kind(config::install_kind().into())
			.with_asset_suffix(if cfg!(target_arch = "aarch64") { "-arm64" } else { "-x64" }),
	);
	let trigger = if automatic { CheckTrigger::Automatic } else { CheckTrigger::Manual };
	ui::run_update_check(updater, frame, UpdateChannel::Stable, trigger);
}

/// Lets a copy that an update relaunched come to the front, which Windows would otherwise open
/// behind whatever the user was doing. Called once the window is first shown.
pub fn come_forward_after_update(frame: &Frame) {
	ui::bring_to_front_after_update(frame);
}
