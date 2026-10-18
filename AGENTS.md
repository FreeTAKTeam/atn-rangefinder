# ATN adapter implementation

Read README.md, docs/map-contract.md, docs/configuration.md and the original research in atn-rangefinder-docs before changing behavior. Apply rust-router and relevant Rust specialized skills.

Rust owns codecs, physical-semantic validation, geometry and synchronization authorization. Android owns Bluetooth/platform permissions, the real trusted REM SDK Binder boundary, and scheduling. Follow RCH's host-owned marker identity/storage model. Do not invent an implemented REM map API or replace map storage with operator telemetry/generic events.

Convention over configuration: everything defaults to the main target, the operator's own ATN Binox 4K (`crates/atn-adapter/src/binox4k-defaults.json`). Overrides are optional, partial JSON in an advanced section, never part of the operator's main flow. Protocol constants and WGS84 mathematical definitions are documented separately. Defaults not yet confirmed on the device (north reference, pitch sign, range type, display slots, firmware) are marked "to confirm" in the defaults' evidence strings and do not block operation. Observer co-location stays an operator yes/no at connect; the Rust validation gates stay in place behind the defaults.

No LXMF/network-core changes, aiming/ballistics/fire control, marker clear-all, write replay/retry on reconnect, or source/binary SDK vendoring. Do not commit credentials, signing keys, proprietary APKs, captures, real locations or device identifiers. Use original synthetic tests only; preserve unrelated local files.

Checks: cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; SDK-pinned Android test/lint/assemble when dependencies are available. Never equate these with real REM map or physical-device acceptance. Record exact unsupported host/hardware gates in docs/implementation-status.md.
