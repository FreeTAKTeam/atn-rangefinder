# goTenna REM plugin: architecture note

Status: design note only. Nothing goTenna-specific is implemented in this repository.

## Shape

- **A separate Android APK and Binder service.** It is its own REM plugin with its own plugin ID,
  manifest capabilities, signing and trust decision in REM. It does not share a process, service,
  queue or GATT object with the ATN plugin.
- **Its own Rust protocol core.** goTenna framing, messaging, queueing and delivery semantics live
  in a goTenna-specific crate set, called through JNI exactly as this plugin calls
  `atn-adapter`. Android owns Bluetooth, permissions, the trusted Binder boundary and scheduling;
  Rust owns codecs, validation and authorization.
- **No shared transport library and no Linux console port in this release.** The two plugins
  agree on conventions, not code. A shared library would couple their release and
  qualification cycles; a desktop/BlueZ port is out of scope.

## Conventions reused from the ATN plugin

- REM's implemented plugin map API: ordinary shared markers through `map.markers.create` with
  plugin-scoped idempotency keys, the observer snapshot, and operator-started selection sessions
  as the only write authorization; host-owned identities are never minted. See
  [`../../map-contract.md`](../../map-contract.md).
- Explicit configuration with `deny_unknown_fields`, unknown semantics blocking conversion and
  writes, and a status snapshot that keeps transport completion, device or recipient reply, and
  operator-confirmed outcome separate. See [`../../configuration.md`](../../configuration.md).
- Per-connection generation IDs, characteristic identity and typed completion/error on the
  native boundary (as in [`../../radio-coexistence.md`](../../radio-coexistence.md)), no write
  replay on reconnect, nothing authorized persisted.
- The shared host-boundary and action/state lifecycle described in the local, uncommitted note
  `docs/plugin-architecture-alignment.md`: the UI issues named actions and renders a state
  snapshot plus lifecycle events; each plugin keeps its own protocol and device semantics.

## What differs from ATN

- **Separate channels.** goTenna needs a reply/indication channel for command acknowledgements
  and a separate status-notification channel for radio state. Each is bound by characteristic
  identity and connection generation, as ATN binds measurements to its notification
  characteristic; neither is interpreted as the other.
- **Messaging semantics stay goTenna's.** Message queueing, retry policy, recipient-confirmed
  delivery and queue advancement are goTenna protocol behaviour with their own evidence
  requirements. They are not modelled on, and must not change, ATN's single-in-flight display
  queue or its no-retry rule.
- **Coexistence.** RNode/goTenna switching is operator-managed with no interlock (see
  [`../../radio-coexistence.md`](../../radio-coexistence.md)); ATN + goTenna on one phone is a
  qualification gate once the goTenna plugin exists.
