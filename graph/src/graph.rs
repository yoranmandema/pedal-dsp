//! The graph: nodes, edges, and the compile pipeline that turns it into an
//! executor.
//!
//! This is the piece from `executor.md`, reduced to what the host simulator
//! needs: validate, topologically sort, plan buffers, emit a flat step list.
//! Cycle resolution and the latency pass are next.

use std::collections::{HashMap, HashSet};

use crate::block::Block;
use crate::types::{BlockError, BlockSpec, PortKind};

pub type NodeId = String;
pub type NodeIndex = usize;

/// An edge: `from` node's output port → `to` node's input port.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    pub from: (NodeId, String),
    pub to: (NodeId, String),
}

/// A node instance in the graph.
pub struct Node {
    pub id: NodeId,
    pub block: Box<dyn Block>,
    pub spec: BlockSpec,
    /// Normalised parameter values (0..1), indexed by spec position.
    pub params: Vec<f32>,
}

impl Node {
    fn new(id: NodeId, block: Box<dyn Block>) -> Self {
        let spec = block.spec();
        let params = spec.params.iter().map(|p| p.normalise(p.default)).collect();
        Self { id, block, spec, params }
    }
}

/// A free-form audio graph. Editable, serialisable, and never run directly.
#[derive(Default)]
pub struct Graph {
    nodes: HashMap<NodeId, Node>,
    edges: Vec<Edge>,
}

/// Why a graph failed to compile.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphError {
    /// A required input port has no incoming edge.
    MissingInput { node: NodeId, port: String },
    /// A port kind mismatch on an edge.
    TypeMismatch { edge: Edge, from: PortKind, to: PortKind },
    /// An edge references a node that does not exist.
    UnknownNode { edge: Edge, which: NodeId },
    /// An edge references a port that does not exist.
    UnknownPort { edge: Edge, node: NodeId, port: String },
    /// A node's `prepare` failed.
    Prepare { node: NodeId, error: BlockError },
    /// The graph has a cycle that is not legal (v1: no cycles at all).
    Cycle(Vec<NodeId>),
    /// Two edges drive the same input port.
    MultipleInputs { node: NodeId, port: String },
    /// The graph needs exactly one `io.input` and one `io.output` node.
    Io(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::MissingInput { node, port } => write!(f, "'{node}.{port}' is not connected"),
            GraphError::TypeMismatch { edge, .. } => write!(
                f,
                "'{}.{}' → '{}.{}' connects different port kinds",
                edge.from.0, edge.from.1, edge.to.0, edge.to.1
            ),
            GraphError::UnknownNode { which, .. } => write!(f, "unknown node '{which}'"),
            GraphError::UnknownPort { node, port, .. } => write!(f, "'{node}' has no port '{port}'"),
            GraphError::Prepare { node, error } => write!(f, "'{node}' failed to prepare: {error}"),
            GraphError::Cycle(nodes) if nodes.is_empty() => write!(f, "the graph has a loop"),
            GraphError::Cycle(nodes) => write!(f, "the graph has a loop through {}", nodes.join(" → ")),
            GraphError::MultipleInputs { node, port } => write!(f, "'{node}.{port}' has more than one wire"),
            GraphError::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node. Returns an error if the id is already taken.
    pub fn add(&mut self, id: impl Into<NodeId>, block: Box<dyn Block>) -> Result<(), BlockError> {
        let id = id.into();
        if self.nodes.contains_key(&id) {
            return Err(BlockError::Other(format!("duplicate node id: {id}")));
        }
        self.nodes.insert(id.clone(), Node::new(id, block));
        Ok(())
    }

    /// Connect an output port to an input port.
    pub fn connect(
        &mut self,
        from_node: &str,
        from_port: &str,
        to_node: &str,
        to_port: &str,
    ) -> Result<(), BlockError> {
        let edge = Edge {
            from: (from_node.to_string(), from_port.to_string()),
            to: (to_node.to_string(), to_port.to_string()),
        };
        self.edges.push(edge);
        Ok(())
    }

    /// Drop every edge driving `node`'s `port`. The graph-side mirror of
    /// [`crate::Doc::disconnect_input`]; an input port takes one wire.
    pub fn disconnect_input(&mut self, node: &str, port: &str) {
        self.edges.retain(|e| !(e.to.0 == node && e.to.1 == port));
    }

    /// Set a parameter to a normalised value.
    pub fn set_param(&mut self, node: &str, param: &str, value: f32) -> Result<(), BlockError> {
        let n = self
            .nodes
            .get_mut(node)
            .ok_or_else(|| BlockError::Other(format!("unknown node: {node}")))?;
        let idx = n
            .spec
            .param_index(param)
            .ok_or_else(|| BlockError::Other(format!("unknown param: {param}")))?;
        n.params[idx] = value.clamp(0.0, 1.0);
        Ok(())
    }

    /// Set a parameter to a real value (denormalised by the spec).
    pub fn set_param_real(&mut self, node: &str, param: &str, value: f32) -> Result<(), BlockError> {
        let n = self
            .nodes
            .get_mut(node)
            .ok_or_else(|| BlockError::Other(format!("unknown node: {node}")))?;
        let idx = n
            .spec
            .param_index(param)
            .ok_or_else(|| BlockError::Other(format!("unknown param: {param}")))?;
        let spec = n.spec.params[idx].clone();
        n.params[idx] = spec.normalise(value);
        Ok(())
    }

    /// Read back the *normalised* (0..1) value of a parameter as stored by
    /// `set_param_real`.
    pub fn param_norm(&self, node: &str, param: &str) -> Option<f32> {
        let n = self.nodes.get(node)?;
        let idx = n.spec.param_index(param)?;
        Some(n.params[idx])
    }

    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.keys().cloned().collect()
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    /// Take a node out of the graph. Used by the executor, which moves blocks
    /// into itself.
    pub fn take_node(&mut self, id: &str) -> Option<Node> {
        self.nodes.remove(id)
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Validate the graph: every required input connected, types match, ports
    /// exist, no cycles.
    pub fn validate(&self) -> Result<(), GraphError> {
        // Ports exist and types match.
        for edge in &self.edges {
            let from = self
                .nodes
                .get(&edge.from.0)
                .ok_or_else(|| GraphError::UnknownNode {
                    edge: edge.clone(),
                    which: edge.from.0.clone(),
                })?;
            let to = self
                .nodes
                .get(&edge.to.0)
                .ok_or_else(|| GraphError::UnknownNode {
                    edge: edge.clone(),
                    which: edge.to.0.clone(),
                })?;

            let from_port = from
                .spec
                .outputs
                .iter()
                .find(|p| p.name == edge.from.1)
                .ok_or_else(|| GraphError::UnknownPort {
                    edge: edge.clone(),
                    node: edge.from.0.clone(),
                    port: edge.from.1.clone(),
                })?;
            let to_port = to
                .spec
                .inputs
                .iter()
                .find(|p| p.name == edge.to.1)
                .ok_or_else(|| GraphError::UnknownPort {
                    edge: edge.clone(),
                    node: edge.to.0.clone(),
                    port: edge.to.1.clone(),
                })?;

            if from_port.kind != to_port.kind {
                return Err(GraphError::TypeMismatch {
                    edge: edge.clone(),
                    from: from_port.kind,
                    to: to_port.kind,
                });
            }
        }

        // At most one edge per input port.
        let mut seen: HashSet<(&str, &str)> = HashSet::new();
        for e in &self.edges {
            if !seen.insert((e.to.0.as_str(), e.to.1.as_str())) {
                return Err(GraphError::MultipleInputs { node: e.to.0.clone(), port: e.to.1.clone() });
            }
        }

        // Exactly one input node and one output node.
        for (kind, label) in [("io.input", "input"), ("io.output", "output")] {
            let n = self.nodes.values().filter(|n| n.spec.id == kind).count();
            if n != 1 {
                return Err(GraphError::Io(format!("the graph needs exactly one {label} node (found {n})")));
            }
        }

        // Every audio input port must be driven.
        for (id, node) in &self.nodes {
            for port in &node.spec.inputs {
                if port.kind != PortKind::Audio {
                    continue;
                }
                let driven = self.edges.iter().any(|e| {
                    e.to.0 == *id && e.to.1 == port.name
                });
                if !driven {
                    return Err(GraphError::MissingInput {
                        node: id.clone(),
                        port: port.name.to_string(),
                    });
                }
            }
        }

        // No cycles in v1.
        if let Some(cycle) = self.find_cycle() {
            return Err(GraphError::Cycle(cycle));
        }

        Ok(())
    }

    /// Find a cycle, if any. Used to reject illegal graphs.
    fn find_cycle(&self) -> Option<Vec<NodeId>> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut stack: HashSet<NodeId> = HashSet::new();
        let mut order: Vec<NodeId> = Vec::new();

        fn dfs(
            node: &NodeId,
            graph: &Graph,
            visited: &mut HashSet<NodeId>,
            stack: &mut HashSet<NodeId>,
            order: &mut Vec<NodeId>,
        ) -> Option<Vec<NodeId>> {
            visited.insert(node.clone());
            stack.insert(node.clone());
            for next in graph.successors(node) {
                if !visited.contains(&next) {
                    if let Some(c) = dfs(&next, graph, visited, stack, order) {
                        return Some(c);
                    }
                } else if stack.contains(&next) {
                    return Some(vec![next.clone(), node.clone()]);
                }
            }
            stack.remove(node);
            order.push(node.clone());
            None
        }

        let ids: Vec<NodeId> = self.nodes.keys().cloned().collect();
        for id in ids {
            if !visited.contains(&id) {
                if let Some(c) = dfs(&id, self, &mut visited, &mut stack, &mut order) {
                    return Some(c);
                }
            }
        }
        None
    }

    /// Nodes that `id` feeds into.
    fn successors(&self, id: &NodeId) -> Vec<NodeId> {
        self.edges
            .iter()
            .filter(|e| e.from.0 == *id)
            .map(|e| e.to.0.clone())
            .collect()
    }

    /// Topological order (Kahn's algorithm).
    pub fn topo_sort(&self) -> Result<Vec<NodeId>, GraphError> {
        let mut indegree: HashMap<NodeId, usize> =
            self.nodes.keys().map(|k| (k.clone(), 0)).collect();
        for e in &self.edges {
            *indegree.get_mut(&e.to.0).unwrap() += 1;
        }

        let mut ready: Vec<NodeId> = indegree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());

        while let Some(n) = ready.pop() {
            order.push(n.clone());
            for succ in self.successors(&n) {
                let d = indegree.get_mut(&succ).unwrap();
                *d -= 1;
                if *d == 0 {
                    ready.push(succ);
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err(GraphError::Cycle(Vec::new()));
        }
        Ok(order)
    }
}
