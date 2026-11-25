//! The preset document: a graph as plain, serialisable data.
//!
//! A [`Graph`] owns live blocks and is consumed by compilation, so it can't
//! be the thing the UI edits or the thing saved to disk. A [`Doc`] is: nodes
//! (id, block type, real-valued params, bypass, canvas position) and edges.
//! Every edit happens on the `Doc`; [`Doc::build`] turns it into a fresh
//! `Graph` to compile.
//!
//! JSON shape:
//!
//! ```json
//! {
//!   "name": "Crunch",
//!   "nodes": [
//!     { "id": "amp", "block": "amp.basic", "params": { "gain": 6.5 }, "pos": [300, 120] }
//!   ],
//!   "edges": [ { "from": ["in", "out"], "to": ["amp", "in"] } ],
//!   "routings": [
//!     { "id": "route1", "source": { "channel": "ch1", "block": "amp" },
//!       "dest": { "channel": "ch2", "block": "delay" },
//!       "enabled": true, "gain": 1.0 }
//!   ]
//! }
//! ```
//!
//! Params are **real** values (dB, ms, %) — what a human reads. Missing params
//! take the block's default; unknown ones are an error, so typos surface.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::models::{ModelResolver, PathResolver};
use crate::registry::Registry;
use crate::types::{BlockSpec, PortKind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub name: String,
    #[serde(default)]
    pub nodes: Vec<NodeDoc>,
    #[serde(default)]
    pub edges: Vec<EdgeDoc>,
    /// Cross-channel routings. Separate from `edges`: a routing taps a block
    /// and adds a feed, it does not replace the block's own wire.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routings: Vec<Routing>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeDoc {
    pub id: String,
    /// Block type id, e.g. `"amp.basic"`.
    pub block: String,
    /// Real parameter values by param id. Missing → the block's default.
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass: bool,
    /// Canvas position for the editor. Ignored by the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 2]>,
    /// Path to a `.nam` model file (for `mod.nam` blocks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Transfer-curve payload (`{"points": [[x, y], ...]}`) for
    /// `dist.waveshaper` blocks. Absent → the block's identity curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<serde_json::Value>,
}

/// A block in a channel: one end of a cross-channel routing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingEndpoint {
    pub channel: String,
    pub block: String,
}

impl RoutingEndpoint {
    pub fn new(channel: impl Into<String>, block: impl Into<String>) -> Self {
        Self { channel: channel.into(), block: block.into() }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_gain() -> f32 {
    1.0
}

/// A cross-channel routing: `source` keeps its normal downstream connection
/// and *additionally* feeds `dest`.
///
/// This is deliberately not an [`EdgeDoc`]. An edge replaces a block's wire;
/// a routing taps a block's output and adds a feed, so the source's original
/// routing survives. Routings live beside `edges` on the [`Doc`] and beside
/// `Channel::sources` on the rig.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Routing {
    /// Stable id, used by `removeRouting` / `setEnabled`.
    pub id: String,
    pub source: RoutingEndpoint,
    pub dest: RoutingEndpoint,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Linear gain applied to the tapped signal. Optional in JSON; a missing
    /// value means unity.
    #[serde(default = "default_gain")]
    pub gain: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeDoc {
    /// `[node, output port]`
    pub from: [String; 2],
    /// `[node, input port]`
    pub to: [String; 2],
}

impl EdgeDoc {
    pub fn new(from: (&str, &str), to: (&str, &str)) -> Self {
        Self {
            from: [from.0.into(), from.1.into()],
            to: [to.0.into(), to.1.into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DocError {
    Json(String),
    UnknownBlock { node: String, block: String },
    UnknownParam { node: String, param: String },
    DuplicateId(String),
    Graph(String),
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocError::Json(e) => write!(f, "preset JSON: {e}"),
            DocError::UnknownBlock { node, block } => write!(f, "'{node}': unknown block type '{block}'"),
            DocError::UnknownParam { node, param } => write!(f, "'{node}': unknown param '{param}'"),
            DocError::DuplicateId(id) => write!(f, "duplicate node id '{id}'"),
            DocError::Graph(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DocError {}

impl Doc {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), nodes: Vec::new(), edges: Vec::new(), routings: Vec::new() }
    }

    pub fn from_json(s: &str) -> Result<Self, DocError> {
        serde_json::from_str(s).map_err(|e| DocError::Json(e.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Doc is always serialisable")
    }

    pub fn node(&self, id: &str) -> Option<&NodeDoc> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut NodeDoc> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Build a fresh, uncompiled [`Graph`]: instantiate every block through
    /// the registry, apply params and wire edges. Validation (missing inputs,
    /// loops, …) happens at compile time.
    ///
    /// Relative `model` paths resolve against the current directory. Use
    /// [`Doc::build_in`] to resolve against the preset's own directory, so a
    /// preset can carry `../models/foo.nam` and still work after the folder is
    /// moved (Mac today, Pi later). [`Doc::build_in_with`] takes a resolver, so
    /// the engine can also accept a model index id.
    pub fn build(&self, reg: &Registry) -> Result<Graph, DocError> {
        self.build_in(reg, std::path::Path::new("."))
    }

    /// As [`Doc::build`], but relative `model` paths resolve against `base`.
    pub fn build_in(&self, reg: &Registry, base: &std::path::Path) -> Result<Graph, DocError> {
        self.build_in_with(reg, base, &PathResolver)
    }

    /// As [`Doc::build_in`], but `model` references are resolved by `resolver`.
    ///
    /// The engine passes an index-aware resolver so a preset can name a model
    /// by its index id; the legacy path behaviour stays available through
    /// [`Doc::build_in`].
    pub fn build_in_with(
        &self,
        reg: &Registry,
        base: &std::path::Path,
        resolver: &dyn ModelResolver,
    ) -> Result<Graph, DocError> {
        let mut g = Graph::new();
        for n in &self.nodes {
            let mut block = reg.build(&n.block).ok_or_else(|| DocError::UnknownBlock {
                node: n.id.clone(),
                block: n.block.clone(),
            })?;
            let spec = block.spec();
            if let Some(path) = &n.model {
                // Absolute paths are used as-is; relative ones are relative to
                // the preset, not to wherever the app was launched from.
                let resolved = resolver.resolve(path, base);
                block
                    .configure("model", &resolved)
                    .map_err(|e| DocError::Graph(e.to_string()))?;
            }
            if let Some(curve) = &n.curve {
                // A corrupt curve is never applied: reject the whole doc.
                block
                    .configure("curve", &curve.to_string())
                    .map_err(|e| DocError::Graph(format!("{}: {e}", n.id)))?;
            }
            g.add(n.id.clone(), block).map_err(|_| DocError::DuplicateId(n.id.clone()))?;
            for (param, &value) in &n.params {
                if spec.param_index(param).is_none() {
                    return Err(DocError::UnknownParam { node: n.id.clone(), param: param.clone() });
                }
                g.set_param_real(&n.id, param, value).map_err(|e| DocError::Graph(e.to_string()))?;
            }
        }
        for e in &self.edges {
            g.connect(&e.from[0], &e.from[1], &e.to[0], &e.to[1])
                .map_err(|e| DocError::Graph(e.to_string()))?;
        }
        Ok(g)
    }

    /// A param's real value, falling back to the spec default.
    pub fn param(&self, node: &str, spec: &BlockSpec, param: usize) -> f32 {
        let p = &spec.params[param];
        self.node(node)
            .and_then(|n| n.params.get(p.id).copied())
            .unwrap_or(p.default)
    }

    // --- editing ---------------------------------------------------------

    /// Add a node of type `block`, with a fresh id derived from the block
    /// name (`amp`, `amp2`, …). Returns the id.
    pub fn add_node(&mut self, block: &str, pos: [f32; 2]) -> String {
        // "time.reverb" → "reverb", but "amp.basic" / "cab.sim" → "amp" / "cab".
        let mut parts = block.splitn(2, '.');
        let (family, kind) = (parts.next().unwrap_or(block), parts.next().unwrap_or(block));
        let stem = match kind {
            "basic" | "sim" | "input" | "output" => family,
            _ => kind,
        }
        .to_string();
        let mut id = stem.clone();
        let mut k = 2;
        while self.node(&id).is_some() {
            id = format!("{stem}{k}");
            k += 1;
        }
        self.nodes.push(NodeDoc {
            id: id.clone(),
            block: block.to_string(),
            params: BTreeMap::new(),
            bypass: false,
            pos: Some(pos),
            model: None, curve: None,
        });
        id
    }

    /// Remove a node and every edge touching it.
    pub fn remove_node(&mut self, id: &str) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from[0] != id && e.to[0] != id);
    }

    /// Connect `from` → `to`. An input takes one wire, so any existing edge
    /// into `to` is replaced.
    pub fn connect(&mut self, from: (&str, &str), to: (&str, &str)) {
        self.disconnect_input(to);
        self.edges.push(EdgeDoc::new(from, to));
    }

    pub fn disconnect_input(&mut self, to: (&str, &str)) {
        self.edges.retain(|e| !(e.to[0] == to.0 && e.to[1] == to.1));
    }

    /// Would `from` → `to` create a loop? (The engine rejects loops in v1;
    /// the editor uses this to refuse the wire up front.)
    pub fn would_loop(&self, from_node: &str, to_node: &str) -> bool {
        // A loop exists if `from_node` is reachable from `to_node`.
        let mut stack = vec![to_node.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(n) = stack.pop() {
            if n == from_node {
                return true;
            }
            if seen.insert(n.clone()) {
                for e in self.edges.iter().filter(|e| e.from[0] == n) {
                    stack.push(e.to[0].clone());
                }
            }
        }
        false
    }

    /// Audio inputs of `node` that have no wire. Handy for error hints.
    pub fn unconnected_inputs(&self, reg: &Registry) -> Vec<(String, String)> {
        let mut v = Vec::new();
        for n in &self.nodes {
            if let Some(spec) = reg.get(&n.block) {
                for p in spec.inputs.iter().filter(|p| p.kind == PortKind::Audio) {
                    if !self.edges.iter().any(|e| e.to[0] == n.id && e.to[1] == p.name) {
                        v.push((n.id.clone(), p.name.to_string()));
                    }
                }
            }
        }
        v
    }
}
