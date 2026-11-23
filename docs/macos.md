# macOS

The tool builds and runs on macOS. It uses BLE through the `btleplug` crate, because macOS has no BlueZ.

## Build

```sh
cargo build --release --no-default-features
```

`--no-default-features` leaves out scrolling text (needs fontconfig) and video (needs libmpv). To include them,
install the libraries (for example with Homebrew) and build with the default features. That build is not tested
on macOS yet.

## Use

```sh
divoom-ditoo-pro-controller devices
divoom-ditoo-pro-controller volume get
divoom-ditoo-pro-controller brightness 50
divoom-ditoo-pro-controller animation images/witch.divoom16
divoom-ditoo-pro-controller status
```

The first Bluetooth command makes macOS ask for Bluetooth permission for your terminal application.

## Differences from Linux

| # | Topic | macOS |
| --- | --- | --- |
| 1 | Device selection | macOS hides the Bluetooth address. The tool selects the device by a part of its BLE name. The default is `DitooPro-Light`. Use `--name PART` if you have more than one device or a different name. |
| 2 | `--device MAC` | Not usable for BLE on macOS. It still applies to the optional version check after a USB flash. |
| 3 | Transport | BLE only. RFCOMM is not available, so `--transport auto` goes directly to BLE. `--transport usb` uses `nusb`, which supports macOS, but USB control is not tested on macOS. |
| 4 | Pairing | Not necessary for BLE. |
| 5 | Write size | Each GATT write is 20 bytes, with a response. Large transfers are slower than on Linux, where the tool can use a larger MTU. |
| 6 | `devices` and `scan` | They list BLE devices in range, with the macOS peripheral UUID in place of an address. |

## Tested on hardware

One Ditoo Pro with stock firmware 306007, from macOS on Apple silicon:

| Command | Result |
| --- | --- |
| `devices` | Found the device by name. |
| `volume get` | Returned the volume. |
| `brightness 100` | Acknowledged, and `status` then reported brightness 100. |
| `animation FILE` | A 5-packet animation was acknowledged and shown. |
| `status` | Returned the decoded reply. |

Not tested on macOS: text, video, firmware update, Lua commands, and USB.

## Code layout

| File | Content |
| --- | --- |
| `src/address.rs` | `Address` is `bluer::Address` on Linux and an equivalent type on other platforms. |
| `src/bluez.rs`, `src/transport/bluez.rs` | The Linux backend (BlueZ, RFCOMM, BLE). The code moved here with no change in behavior. |
| `src/portable.rs`, `src/transport/portable.rs` | The backend for platforms with no BlueZ. |
| `src/transport.rs` | The shared part: transport selection, the packet envelope, and reply parsing. |

To support a new platform or Bluetooth library, add a backend that provides `BleConnection` with `connect`,
`send`, and `send_inner`, and select it with a `cfg` attribute in `src/transport.rs`.
