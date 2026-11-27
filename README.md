# Headroom

A small Windows app that reads the battery level directly from a SteelSeries Arctis Nova Pro Wireless, Nova Elite or Nova 5 base station, or a Nova 7X dongle, and reads it to you from anywhere with a hotkey. SteelSeries GG is not required.

## Use

Connect the base station or dongle by USB and launch `headroom.exe`. The window shows the headset battery level, and the spare battery's level for the Nova Pro Wireless and Nova Elite, refreshed every five seconds.

- Press Alt+Shift+B from any program to hear the current levels through your screen reader, or a system voice if none is running. Braille displays get them too.
- Press Escape to hide Headroom in the notification area. Click its icon, choose Show/Hide from its menu, or launch it again, to bring the window back.
- Choose Change Hotkey in the window to pick a different hotkey, or none. It's saved in `headroom.toml`, beside the executable, or in `%APPDATA%\Headroom` for an installed copy.
- Choose Check for Updates to look for a newer release. Headroom also looks each time it starts, and asks before installing anything.
- Choose Exit, in the window or the notification area menu, or close the window, to quit.

If the base station or dongle is disconnected, the window shows an error and keeps retrying. An offline headset is shown as off or out of range, while the Nova Pro's dock reading remains available. If another program already uses the hotkey, Headroom says so when it starts.

## Supported hardware

- Tested: Nova Pro Wireless base station, USB vendor/product `1038:12e0`, on Windows x64, including with GG and Engine stopped.
- Tested: Arctis Nova 7X, USB vendor/product `1038:2206`. It has no spare battery, so only the headset is shown.
- Tested: Arctis Nova Elite base station, USB vendor/product `1038:2244`.
- Tested: Arctis Nova 5 base station, USB vendor/product `1038:2232`. It has no spare battery, so only the headset is shown.
- Recognized but not hardware-tested: Nova Pro Wireless X base station (`1038:12e5`), Nova Elite Sage & Gold (`1038:2270`), and Nova 5X base station (`1038:2253`).
- Not supported: the Xbox edition of the Nova Pro Wireless (`1038:225e`). Windows runs it as an Xbox controller rather than a HID device, so Headroom can't query it.
- Other product ids and headset families are not currently supported, including the other Nova 7 and 7X revisions, some of which report a different battery scale. If several supported devices are connected, the first accessible status interface is used.

The Nova Pro base station reports battery levels in steps of 12.5%, and the Nova 7X in steps of 25%, rather than exact percentages. Intermediate values are labeled “About”; 100% means the highest reported step. The Nova Elite and Nova 5 report exact percentages, which are shown as they are. The Nova Pro dock's charging/charged state is inferred from its level. A zero dock reading is treated as an empty slot, but a very low battery may be indistinguishable from an absent one.

## Build

Use the stable Rust MSVC toolchain, Visual Studio Build Tools with Desktop development with C++ (a C++23 compiler, for prism's speech library), CMake 3.24 or later, and LLVM/libclang for wxDragon's bindings. Make those tools available to your build environment; if bindgen cannot locate libclang, set `LIBCLANG_PATH` to the directory containing `libclang.dll`.

```
cargo build --release --locked
```

The first build downloads wxWidgets sources and compiles the native GUI and speech components. Building requires internet access unless those sources and the Rust dependencies are cached.

Headroom's only network request is the update check, to GitHub's releases API. Updates are verified with a minisign signature before they're installed. To turn off the check at startup, add `check_for_updates = false` to `headroom.toml`.

## Development

```
cargo +nightly fmt
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked
```

The ordinary tests use status-report fixtures and do not require hardware. To run the optional live battery query with a connected base station:

```
cargo test --locked live_hid_status -- --ignored --nocapture
```

See [findings.md](findings.md) for the USB protocol and verification notes. This is an independent project, unaffiliated with SteelSeries.

## Releasing

1. Set the version in `Cargo.toml`, and add a `## <version>` section to `CHANGELOG.md`.
2. Tag the commit `v<version>` and push the tag.

The release workflow builds x64 and ARM64 zips and installers, signs each with the minisign key in the `MINISIGN_KEY` and `MINISIGN_PASSWORD` secrets, and publishes the release, with that changelog section as its notes. To package locally, run `cargo xtask release`; without those variables set, the files aren't signed and the updater won't install them.

## License

[MIT](LICENSE).
