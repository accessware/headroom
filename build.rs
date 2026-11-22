use std::path::Path;

use shipfitter::{
	build::{configure_file, target_profile_dir},
	windows::{VersionInfo, embed_manifest},
};

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	println!("cargo:rerun-if-changed=Cargo.toml");
	println!("cargo:rerun-if-changed=headroom.iss.in");
	embed_manifest("Headroom").expect("Could not embed the Windows application manifest");
	VersionInfo {
		product_name: "Headroom",
		company: "Quin Gillespie",
		copyright: "Copyright © 2026 Quin Gillespie",
		original_filename: "headroom.exe",
		..VersionInfo::default()
	}
	.embed()
	.expect("Could not embed the Windows version information");
	// The installer script for `cargo xtask release`, beside the executable it installs, with this
	// build's version and architecture filled in.
	if let Some(target_dir) = target_profile_dir() {
		configure_file(Path::new("headroom.iss.in"), &target_dir.join("headroom.iss"), &[])
			.expect("Could not write the installer script");
	}
}
