# Direct USB battery protocol

Headroom queries the base station or dongle over Windows HID. It does not need SteelSeries GG, Engine, a local HTTP API, or a custom USB driver.

Each supported headset family is a profile in `src/battery.rs`: its product ids, USB interface, report size, query header, and decoder. The sections below describe the Nova Pro Wireless; [the Nova 7X](#arctis-nova-7x) follows them.

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

## Arctis Nova 5X

SetupAPI and `HidP_GetCaps` enumerated these top-level collections on the `1038:2253` dongle on Windows x64:

| USB interface | Usage page | Usage | Input bytes | Output bytes |
| --- | --- | --- | ---: | ---: |
| `MI_03` | `FFC0` | `0001` | 65 | 65 |
| `MI_04` | `000C` | `0001` | 5 | 0 |
| `MI_05` | `FF00` | `0001` | 65 | 65 |

Before the Nova 5X profile was added, the product ID check skipped every collection before reading its capabilities. The new profile selects `MI_03` and checks its usage page, usage, and report lengths. A 65-byte `WriteFile` request starting `00 B0` and padded with zeros received a 65-byte `ReadFile` reply on that collection. Only `MI_03` was queried.

| Windows byte offset | Meaning |
| --- | --- |
| 0 | Zero report ID slot |
| 1 | Status command `B0` |
| 2 | Connection state, `03` online and `02` offline |
| 3 | Coarse battery step |
| 4 | Battery percentage, 0–100 |
| 5 | Charge state while online, `01` charging and `03` on battery |

| Hardware state | Windows reply prefix | Reading |
| --- | --- | --- |
| On, USB charging cable attached | `00 B0 03 02 42 01 64` | 66%, charging |
| On, after unplugging the cable | `00 B0 03 02 44 03 64` | 68%, on battery |
| On, later sample | `00 B0 03 02 4B 03 64` | 75%, on battery |

The rest of these replies was zero. HeadsetControl 4.3.0 agreed with both on-battery readings on the same Windows machine. Its Nova 5/5X decoder indexes the HID payload, so its percentage offset 3 is Windows offset 4. The external capture `B0 03 03 61 03 64 64 00` follows the same layout. `61` is 97%, and the adjacent `03` is the coarse step. GG and Engine were stopped during the local readings. [HeadsetControl Nova 5/5X decoder](https://github.com/Sapd/HeadsetControl/blob/4.3.0/lib/devices/steelseries_arctis_nova_5.hpp) · [External capture](https://github.com/CachyOS/distribution/issues/628)

On 2026-11-24, powering off the headset with the dongle still connected changed the 65-byte reply prefix from `00 B0 03 02 0C 03 64` to `00 B0 02 02 0C 03 64`. The dongle kept reporting the last 12% and on-battery codes after disconnecting. Headroom displayed “Off or out of range,” and HeadsetControl 4.3.0 reported `BATTERY_UNAVAILABLE`. Only PID `2253` was tested.

## References

- [HeadsetControl device protocol](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_pro_wireless.hpp): product ids, interface number, status request, headset battery scale and state codes.
- [HeadsetControl Nova 7 protocol](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_7.hpp): the Nova 7 and 7X product ids, interface number, status request, and battery scales.
- [HeadsetBatteryTray protocol notes](https://github.com/Teknesyum/HeadsetBatteryTray#how-it-works): Windows collection selection and report format.
- [Rigstats control architecture notes](https://github.com/dvalfrid/rigstats/blob/main/docs/control-architecture.md): spare battery byte and scale, marked unverified there.

These projects were consulted for protocol facts. This repository contains an independently written Windows transport and parser; it does not bundle their implementations. The initial GG HTTP approach has been removed from the app.
