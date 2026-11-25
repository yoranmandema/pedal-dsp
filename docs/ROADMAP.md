# Roadmap

The `pedal` Kanban board is the execution source of truth; this file records
product direction and groups the active work. The current product path is
**Mac-first Preact/Tauri UI → Raspberry Pi 5 deployment**.
The egui app remains a developer/debug surface. See `UI-ARCHITECTURE.md`,
`IPC-PROTOCOL.md`, and `HANDOFF.md` for architecture and verified state.

## Direction and completed foundations

- The Mac concept rig, DSP/graph executor, NAM playback, pitch-delay and
  continuous sweep are implemented; corresponding historical US cards are
  done or archived.
- The Preact UI, loopback WebSocket engine bridge, Tauri desktop bundle, and
  baseline pedalboard UI stories are done. This is a native webview app, not a
  Rust-hosted served page.
- The active near-term work is UI usability and polish on the Mac. Current
  work includes bottom-editor layout/responsiveness and block-picker/drag
  interactions. Check live card statuses and dependencies before dispatching;
  this roadmap does not override them.

## Next product phase: improve the Mac UI

Prioritize small, observable UI slices that can be built and visually checked
on macOS without hardware. The live board currently contains bottom-panel
responsiveness work (`t_d42c3156`, running; `t_8ed177cc` and `t_f25bee02`
ready) followed by visual QA (`t_d2973e1f`, todo). A redesign proposal and
cross-channel routing are separate later efforts; do not gate routine panel
polish on them. Avoid assigning two UI cards to one profile concurrently.

Other active UI candidates include block insertion via drag/drop and the
Add Block modal. Keep their dependencies limited to concrete component/API
prerequisites. Do not dispatch duplicates of done cards or treat speculative
TONE3000 integration, plugin support, or hardware design as prerequisites for
Mac UI work.

The instrument tuner (`t_b366a8fb`) is decomposed into an engine card (YIN pitch
detection on the input, `event: tuner`) and a UI card (top-bar button + modal),
UI behind engine. The design is frozen in
`docs/design/instrument-tuner.md`; the tuner is a read-only stream, so it adds
no command and no state field.

## Pi 5 deployment (later, hardware-dependent)

After the Mac UI and engine behavior are stable, validate Linux/Pi audio I/O,
round-trip latency and CPU, then package the fullscreen touchscreen app. Keep
hardware-dependent measurement cards parked until the Pi and suitable audio
interface are available. Do not represent those tests as passed based on Mac
benchmarks. The Pi 5 is the settled hardware direction; firmware work
is dormant and must not be reopened absent an explicit product-owner change.

## Parked / decision-gated areas

- **Physical hardware design:** analogue front end, ADC/DAC, controls, enclosure
  and display need product-owner/hardware decisions and/or hardware access.
- **Plugin:** wrapper, plugin UI and automation are exploratory, not on the
  current critical path. Revisit framework/licensing choices before dispatch.
- **TONE3000:** API reconnaissance and UX/schema work exist, but network/auth,
  model licensing and download policy should be resolved before implementation.
- **Cross-channel routing:** the data model, persistence and IPC surface are
  decided (`docs/design/cross-channel-routing.md`, landed with `t_21473ff1`);
  the engine fan-out and UI cards are queued behind it. Do not make unrelated
  UI tasks depend on the routing fan-in.

## Board hygiene

Use the live `pedal` board for status, assignment and dependency edges. Mark
completed historical cards done only with implementation evidence; archive
superseded duplicates rather than claiming work completed. Hardware-dependent
or undecided cards should remain explicitly parked/unassigned, not dispatchable.
Each builder card should state scope, observable acceptance checks, verification
commands and a product-owner try-it path. `AGENTS.md` defines role ownership,
worktree, review and landing rules.
