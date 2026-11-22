//! `cargo xtask release`: builds Headroom and packages it for a GitHub release.

use std::{
	env,
	error::Error,
	path::{Path, PathBuf},
	process::ExitCode,
};

use shipfitter::{
	host_arch_suffix,
	package::{Zip, cargo_build_release, inno_setup},
	sign::Minisign,
};

const EXE_NAME: &str = "headroom.exe";

fn main() -> ExitCode {
	let result = if env::args().nth(1).as_deref() == Some("release") {
		release()
	} else {
		println!("Tasks:");
		println!("	release    Build Headroom in release mode, then package it as a zip and an installer");
		Ok(())
	};
	// Printed here rather than returned from main, which would print it with Debug, in quotes.
	if let Err(error) = result {
		eprintln!("Error: {error}");
		return ExitCode::FAILURE;
	}
	ExitCode::SUCCESS
}

/// Builds Headroom, then packages it in `target/release`, named for the machine's architecture,
/// such as `x64`: `headroom-x64.zip`, a portable copy that keeps its settings beside it, and
/// `headroom_setup-x64.exe`, which installs it. With the minisign key in `MINISIGN_KEY` and its
/// password in `MINISIGN_PASSWORD`, each gets a `.minisig` signature beside it, which the updater
/// checks before installing anything.
fn release() -> Result<(), Box<dyn Error>> {
	// The key is read before anything is built, so a broken secret fails at once.
	let minisign = Minisign::from_env()?;
	let artifacts = package()?;
	match minisign {
		Some(minisign) => {
			for artifact in &artifacts {
				println!("Signed {}", minisign.sign(artifact)?.display());
			}
		}
		None => println!("MINISIGN_KEY isn't set, so nothing is signed, and the updater won't install these."),
	}
	Ok(())
}

/// Builds and packages Headroom, returning the files made.
fn package() -> Result<Vec<PathBuf>, Box<dyn Error>> {
	let root = project_root();
	cargo_build_release(&root, &["headroom"])?;
	let target_dir = root.join("target/release");
	let exe = target_dir.join(EXE_NAME);
	if !exe.exists() {
		return Err(format!("{} wasn't built", exe.display()).into());
	}
	let arch = host_arch_suffix();
	let zip_path = target_dir.join(format!("headroom-{arch}.zip"));
	let mut zip = Zip::create(&zip_path)?;
	zip.file(&exe, EXE_NAME)?;
	zip.file(&root.join("README.md"), "README.md")?;
	zip.file(&root.join("LICENSE"), "LICENSE.txt")?;
	zip.finish()?;
	println!("Created zip: {}", zip_path.display());
	let mut artifacts = vec![zip_path];
	// The build script wrote the installer script beside the executable. Without Inno Setup this
	// only warns, since the zip is already made.
	if inno_setup(&target_dir.join("headroom.iss"))? {
		let installer = target_dir.join(format!("headroom_setup-{arch}.exe"));
		println!("Created installer: {}", installer.display());
		artifacts.push(installer);
	}
	Ok(artifacts)
}

fn project_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives one level below the project").to_path_buf()
}
