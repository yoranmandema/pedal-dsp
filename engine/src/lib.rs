//! The pedal engine: the real-time audio core, the channel model, presets, and
//! the IPC server the UI talks to.
//!
//! The engine is a **library** plus a **process**:
//!
//! * [`audio`] owns the executor and runs it in the audio callback.
//! * [`channels`] is the channel/block model the UI edits.
//! * [`store`] is the preset directory.
//! * [`host`] picks devices and builds the cpal streams.
//! * [`server`] speaks the protocol in `docs/IPC-PROTOCOL.md`.
//!
//! The process (`src/main.rs`) runs the audio thread, binds a loopback
//! WebSocket, and keeps both in sync.

pub mod audio;
pub mod channels;
pub mod headless;
pub mod host;
pub mod lifecycle;
pub mod registry;
pub mod models;
pub mod model_index;
pub mod tuner;
pub mod server;
pub mod store;
