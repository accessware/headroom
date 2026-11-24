# Direct USB battery protocol

Headroom queries the base station or dongle over Windows HID. It does not need SteelSeries GG, Engine, a local HTTP API, or a custom USB driver.

Each supported headset family is a profile in `src/battery.rs`: its product ids, USB interface, report size, query header, and decoder. The sections below describe the Nova Pro Wireless; [the Nova 7X](#arctis-nova-7x) and [the Nova Elite](#arctis-nova-elite) follow them.

## Find the status interface

Enumerate present HID device interfaces with SetupAPI. Filter device paths for vendor `1038`, product `12e0` (Nova Pro Wireless) or `12e5` (Wireless X), USB interface `04`.

Confirm the vendor/product with `HidD_GetAttributes`, then inspect HID capabilities. Use the collection with usage page `0xffc0`, usage `1`, and 64-byte input/output reports. Do not use the separate `0xff00` notification collection.

Open the status interface with shared read/write access and overlapped I/O. The implementation rediscovers and opens it on each poll, so a disconnected or reconnected station does not leave a stale persistent handle.

## Query and decode

Write this 64-byte output report:

```text
06 B0 00 00 ... (remaining bytes zero)
```

This is a status query. No save, reset, configuration, or firmware commands are sent.

Read input reports until a matching `06 B0` header arrives, with an overall 1.5-second read deadline. Require at least 16 bytes before decoding:

| Zero-based byte offset | Meaning |
| --- | --- |
| 0 | Report id `06` |
| 1 | Status command `B0` |
| 6 | Headset battery step, 0–8 |
| 7 | Dock/spare battery step, 0–8 |
| 15 | Headset connection/charging state |

Known headset states:

| Value | Display |
| --- | --- |
| `01` | Off or out of range; do not display the cached headset level |
| `02` | Charging |
| `08` | On battery |

Other state values are displayed as unavailable with their raw code; they are not assumed to mean connected. Battery codes outside 0–8, truncated frames, and unrelated reports are rejected.

Percentages are integer `step * 100 / 8`, giving 0, 12, 25, 37, 50, 62, 75, 87, and 100. Intermediate readings are labeled “About” because this is a coarse battery gauge, not a continuous measurement.

The dock state follows the convention used by GG: zero is treated as no battery, steps 1–7 as charging, and step 8 as charged. This state is inferred from the dock's level; the headset state byte must not be applied to the spare. A very low spare may be indistinguishable from an absent battery.

Both reads and writes use timed overlapped I/O. A pending operation is cancelled and its completion awaited before releasing its buffer or event handle. Device and event handles, preparsed capability data, and the SetupAPI enumeration set are released on every exit path.

## Local verification

Verified on 2026-11-06 with the `1038:12e0` base station:

```text
06 b0 00 00 01 00 03 08 08 01 02 0a 05 00 08 08 ...
```

The remaining bytes were zero. The independent Python probe and Rust reader both decoded headset step 3 (about 37%), spare step 8 (100%), and headset state `08` (on battery).

After the user quit GG, process enumeration found neither GG nor Engine running. The Rust hardware test returned the same readings with both processes absent. Native window inspection also verified the two-line display and both Close and Escape behavior. No audio or device settings were changed.

Intermediate dock levels, removal/reinsertion, headset-off behavior, and the X variant have parser coverage or protocol references, but were not physically exercised during this session. They remain useful hardware checks for future contributors.

## Arctis Nova 7X

Vendor `1038`, product `2206`, USB interface `03`, usage page `0xffc0`, usage `1`. Reports are 65 bytes as Windows sees them: a report id of `00` (the device doesn't use ids) followed by a 64-byte payload.

Write `00 B0 00 ...` (65 bytes) and read until a report starting `00 B0` arrives, with the same 1.5-second deadline as above. Offsets are zero-based in the 65-byte report:

| Offset | Meaning |
| --- | --- |
| 0 | Report id `00` |
| 1 | Status command `B0` |
| 3 | Headset battery level, 0–4 (0, 25, 50, 75, 100%) |
| 4 | Headset state |

Known states: `00` offline (the level isn't shown or validated), `01` and `02` charging, `03` on battery. Anything else is displayed as unavailable with its raw code. There is no spare battery.

Verified on 2026-11-22 with a `1038:2206` dongle. The reply was `00 b0 03 03 03 64 64 00 00 00 2e 30 30 ...`. The headset was at 75% and not charging, as GG showed, so level `03` is 75% on this scale. Bytes 5 and 6 (`64 64`) are not decoded. The rest of the reply looks like an ASCII firmware version. Charging, offline, and other charge levels have parser coverage only.

HeadsetControl lists other Nova 7 and 7X product ids (`22a4` with the same discrete scale, `22a5`, `2258`, `229e`, `22ad` and others with a direct 0–100 percentage). Headroom recognizes only `2206`, until those are verified on hardware.

## Arctis Nova Elite

Vendor `1038`, product `2244` (`2270` for the Sage & Gold edition, untested), USB interface `03`. That interface has two vendor collections. `col01` has usage page `0xffc0`, usage `1` and 64-byte input/output reports, and carries the status query and its reply. `col02` (usage page `0xff00`) carries the base station's own event reports, and isn't used. Both collections match the profile's device path, and the capability check selects `col01`.

Write `01 B0 00 ...` (64 bytes) on `col01`, and read until a report starting `01 B0` arrives, with the same 1.5-second deadline as above. The layout is the Nova Pro's, but the levels are exact percentages:

| Offset | Meaning |
| --- | --- |
| 0 | Report id `01` |
| 1 | Status command `B0` |
| 6 | Headset battery, 0–100% |
| 7 | Spare battery in the base station's charger, 0–100% |
| 15 | Headset state, with the Nova Pro's codes |

Levels above 100 are rejected. The spare is treated as on the Nova Pro: 0 means no battery, 100 means charged, and anything between means charging.

Verified on 2026-11-24 with a `1038:2244` base station. Its `col01` capabilities were read live, and the Rust hardware test returned headset 57% on battery and spare 100% charged. Replies captured by the earlier steelcage project include:

```text
01 b0 00 00 01 00 59 64 08 00 00 0a 00 00 08 08 03   headset 89%, on battery; spare 100%
01 b0 00 00 01 00 64 57 08 00 00 0a 00 00 08 08 03   headset 100%, on battery; spare 87%
01 b0 00 00 01 00 00 64 08 00 00 0a 00 00 04 01 03   headset off (level reads 0); spare 100%
```

States `08` (on battery) and `01` (off) were observed. `02` (charging) is assumed from the Nova Pro, but hasn't been seen on an Elite. Bytes 14 and 16 aren't decoded.

The Xbox edition of the Nova Pro Wireless (`1038:225e`) isn't a HID device on Windows. It's driven by the Xbox controller driver (`dc1-controller`) with generic USB audio, and has no HID interfaces to query.

## References

- [HeadsetControl device protocol](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_pro_wireless.hpp): product ids, interface number, status request, headset battery scale and state codes.
- [HeadsetControl Nova 7 protocol](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_7.hpp): the Nova 7 and 7X product ids, interface number, status request, and battery scales.
- [HeadsetBatteryTray protocol notes](https://github.com/Teknesyum/HeadsetBatteryTray#how-it-works): Windows collection selection and report format.
- [Rigstats control architecture notes](https://github.com/dvalfrid/rigstats/blob/main/docs/control-architecture.md): spare battery byte and scale, marked unverified there.

These projects were consulted for protocol facts. This repository contains an independently written Windows transport and parser; it does not bundle their implementations. The initial GG HTTP approach has been removed from the app.
