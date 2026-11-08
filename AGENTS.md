# Repository guidance

## Purpose

This is a portable Miryoku firmware sample for the 36-key REMAKE keyboard,
using nice!nano v2 controllers and nice!view displays. It is an example for
REMAKE users, not the maintainer's preferred layout or a full QMK/ZMK port.
Read `README.md` for the layout, adaptation limits, hardware and flashing details.

## Sources and constraints

- `keyboard.toml` defines the seven-layer Colemak Mod-DH keymap, tap/hold
  behavior, matrix pins and split configuration. `vial.json` maps the 36
  physical keys for remapping; keep these and layout documentation consistent.
- `src/central.rs` is the left/central firmware; `src/peripheral.rs` is the
  right/peripheral firmware. Shared modules implement displays, SPI, clocks
  and USB-power detection.
- Firmware and `tests/host/Cargo.toml` both use released RMK `0.9.x`.
  Allow compatible patch releases and keep both lockfiles on the same RMK
  version when updating. Do not introduce Git dependencies or local overrides.
- Preserve the 4 × 5 matrix per half, `col2row` diodes, physical thumb columns
  2/3/4, reversed right scan columns and nice!nano D18 mapping to P1.15.
- Keep changes focused and retain Miryoku attribution. Update `README.md`
  when changing user-visible behavior or the sample's limitations.

## Checks

Run from the repository root after firmware or configuration changes:

```sh
cargo make test
cargo build --release --bins --locked
```

`cargo make test` selects the native host target; the repository's default
target is `thumbv7em-none-eabihf`. Use `cargo make uf2 --release` to produce
both flash images. Native tests do not validate electrical continuity,
physical timing or radio behavior; report hardware checks separately.
