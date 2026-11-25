//! `dsp-graph` — a free-form audio graph: nodes, edges, compile pipeline,
//! executor.
//!
//! Platform-independent: no ALSA, no CoreAudio, no I/O. The same code runs on
//! macOS, Linux, and the Pi.

pub mod blocks;
pub mod overdrive;
pub mod fuzz;
pub mod waveshaper;
pub mod doc;
pub mod block;
pub mod executor;
pub mod graph;
pub mod models;
pub mod registry;
pub mod types;

pub use block::{Block, BlockCtx};
pub use doc::{Doc, DocError, EdgeDoc, NodeDoc, Routing, RoutingEndpoint};
pub use executor::{CompileError, Executor, MAX_PORTS};
pub use graph::{Edge, Graph, GraphError, NodeId};
pub use models::{IndexResolver, ModelIndex, ModelRecord, ModelResolver, PathResolver};
pub use registry::Registry;
pub use types::{
    BlockError, BlockSpec, Category, Curve, ParamId, ParamSpec, PortKind, PortSpec, Unit,
};

/// Convenience for block authors.
pub mod prelude {
    pub use crate::block::{Block, BlockCtx};
    pub use crate::types::{BlockError, BlockSpec, Category, Curve, ParamId, ParamSpec, PortKind, PortSpec, Unit};
}
