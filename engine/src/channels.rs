//! The channel model: what the UI edits.
//!
//! A **channel** is an ordered strip of blocks. Routing says which other
//! channels feed it. The engine underneath still runs a `Doc` graph, so
//! nothing is lost: [`Rig::to_doc`] lays the channels out as a graph (each
//! channel gets a `Split` node so it can be fed by several sources), and
//! [`Rig::from_doc`] reads that layout back.
//!
//! A **cross-channel routing** ([`Routing`]) is a different thing: it taps one
//! block's output and *additionally* feeds a block in another channel. It lives
//! in [`Rig::routings`], not in a channel's `sources` and not in the graph's
//! `edges`, so the source block's own downstream connection is untouched. It
//! carries an `enabled` flag and a `gain`, and it is saved in the project file
//! beside the edges.
//!
//! is the same idea with three.
//!
//! A rig always has exactly [`CHANNEL_COUNT`] channels. They are slots, not
//! something the user adds or removes: an empty one is simply left out of the
//! compiled graph ([`Rig::to_doc`]), so it costs nothing but still appears in
//! the UI. With one channel populated this is a pedalboard; with two it is a
//! parallel rig: channel B's source row has `A` and `In` ticked. A send/return
//! is the same idea with three.
//!
//! A rig with no blocks at all is a bypass, not a dead pedal: [`Rig::has_content`]
//! is the fast-path test the engine uses to skip compiling a graph for it.

use std::collections::BTreeMap;

use dsp_graph::{Doc, EdgeDoc, Graph, NodeDoc, Registry, Routing, RoutingEndpoint};

/// The number of channel slots a rig always has.
pub const CHANNEL_COUNT: usize = 4;
/// The pedal's input, as a routing source.
pub const IN: &str = "<in>";

/// Node id prefix for the summing node a routing inserts at its destination.
/// Reserved: block ids come from palette display names, which never start with
/// `__`, so these cannot collide with a block.
pub const ROUTING_MIX_PREFIX: &str = "__routing_";
/// Node id prefix for a routing's gain stage.
pub const ROUTING_GAIN_PREFIX: &str = "__routing_gain_";

/// Number of grid columns available per channel.
pub const GRID_COLS: u8 = 8;

#[derive(Clone, Debug)]
pub struct Channel {
    pub id: String,
    pub name: String,
    /// Block instances, in order. A `util.master` is the channel's output and
    /// is never a strip block; it lives in `master_volume` / `master_ceiling`.
    pub blocks: Vec<NodeDoc>,
    /// Source channel ids; `IN` means the pedal input.
    pub sources: Vec<String>,
    /// The channel's output volume, dB.
    pub master_volume: f32,
    /// The channel's limiter ceiling, dB.
    pub master_ceiling: f32,
}

impl Channel {
    pub fn new(id: &str, name: &str) -> Self {
        Self { id: id.into(), name: name.into(), blocks: Vec::new(), sources: vec![IN.into()], master_volume: 0.0, master_ceiling: -1.0 }
    }

    /// Ensure every block has a column, all columns are unique and `< GRID_COLS`,
    /// and blocks are sorted by column. Blocks without a col (legacy preset) get
    /// the lowest free column in their current order.
    pub fn normalize_cols(&mut self) -> Result<(), String> {
        if self.blocks.len() > GRID_COLS as usize {
            return Err(format!("channel {} has more than 8 blocks", self.id));
        }
        // Collect existing (col, index) pairs, sort by col to find gaps.
        let mut cols: Vec<Option<u8>> = Vec::new();
        for b in &self.blocks {
            cols.push(b.col);
        }
        // Assign missing cols.
        let mut used = std::collections::HashSet::new();
        for c in &cols {
            if let Some(c) = c {
                used.insert(*c);
            }
        }
        for b in &mut self.blocks {
            if b.col.is_none() {
                let mut found = false;
                for c in 0..GRID_COLS {
                    if !used.contains(&c) {
                        b.col = Some(c);
                        used.insert(c);
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(format!("channel {} has more than 8 blocks", self.id));
                }
            }
        }
        // Sort blocks by column.
        self.blocks.sort_by_key(|b| b.col.unwrap_or(0));
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Rig {
    pub name: String,
    pub channels: Vec<Channel>,
    /// Cross-channel routings. This is a separate list from each channel's
    /// `sources` and from the block chain: a routing taps a block's output and
    /// *additionally* feeds a block in another channel, so the source keeps
    /// its original downstream connection(s).
    pub routings: Vec<Routing>,
    /// Pedal input trim, dB. Applied by the `io.input` node, before any channel.
    pub input_gain: f32,
    /// Pedal output volume, dB. Applied by the output limiter node.
    pub master_volume: f32,
}

impl Default for Rig {
    fn default() -> Self {
        Self::new("")
    }
}

/// The master node id for a channel.
pub fn master_id(ch: &str) -> String {
    format!("{ch}__master")
}

/// Id of the pedal's global output limiter. Reserved: `Rig::from_doc` uses it
/// to tell the output node apart from a channel master, which is also a
/// `util.master`.
pub const OUT_MASTER_ID: &str = "out__master";

/// Would adding `source` → `dest` close a feedback loop?
///
/// The block-level signal graph is the channel chains, the channel-source
/// feeds, and the enabled routings. A cycle exists when `dest` can already
/// reach `source` through it.
fn routing_creates_cycle(rig: &Rig, source: &RoutingEndpoint, dest: &RoutingEndpoint) -> bool {
    let edges = rig.signal_edges(Some((&source.block, &dest.block)));
    reaches(&edges, &dest.block, &source.block)
}

/// Depth-first reachability over block-level signal edges. Used to refuse a
/// routing that would close a feedback loop.
fn reaches(edges: &[(String, String)], from: &str, to: &str) -> bool {
    let mut stack = vec![from.to_string()];
    let mut seen = std::collections::HashSet::new();
    while let Some(n) = stack.pop() {
        if n == to {
            return true;
        }
        if seen.insert(n.clone()) {
            stack.extend(edges.iter().filter(|(s, _)| *s == n).map(|(_, d)| d.clone()));
        }
    }
    false
}

impl Rig {
    /// A rig with the full [`CHANNEL_COUNT`] channel slots, all empty.
    pub fn new(name: &str) -> Self {
        let channels = (1..=CHANNEL_COUNT)
            .map(|n| Channel::new(&format!("ch{n}"), &format!("Channel {n}")))
            .collect();
        Self {
            name: name.into(),
            channels,
            routings: Vec::new(),
            input_gain: 0.0,
            master_volume: 0.0,
        }
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// Whether any channel holds a block.
    ///
    /// The empty-rig fast path: a rig of nothing but empty slots compiles to the
    /// bypass, not to a graph, so an untouched pedalboard costs no compile and
    /// no blocks are instantiated. A single block anywhere flips this, and from
    /// then on the rig is compiled normally — including any still-empty slots,
    /// which simply stay out of the graph.
    pub fn has_content(&self) -> bool {
        self.channels.iter().any(|c| !c.blocks.is_empty())
    }

    pub fn channel_mut(&mut self, id: &str) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    pub fn id_taken(&self, id: &str) -> bool {
        self.channels.iter().any(|c| c.blocks.iter().any(|b| b.id == id))
    }

    /// Add a block to a channel's strip. Returns its id.
    pub fn add_block(&mut self, channel: &str, block: &str, pos: [f32; 2]) -> String {
        let stem = block.rsplit('.').next().unwrap_or(block).to_string();
        self.add_block_as(channel, block, &stem, pos, None).unwrap_or_else(|e| e)
    }

    /// As [`Rig::add_block`], but the node is named after the block's display
    /// name rather than the block type's last segment. Picking "Amp" from the
    /// palette should not produce a node called "basic".
    pub fn add_block_as(
        &mut self,
        channel: &str,
        block: &str,
        name: &str,
        pos: [f32; 2],
        col: Option<u8>,
    ) -> Result<String, String> {
        let stem = name.trim().to_lowercase();
        let stem = if stem.is_empty() {
            block.rsplit('.').next().unwrap_or(block).to_string()
        } else {
            stem
        };
        let mut id = stem.clone();
        let mut k = 2;
        while self.id_taken(&id) {
            id = format!("{stem}{k}");
            k += 1;
        }
        // Determine and validate column.
        let ch = self.channel_mut(channel).ok_or_else(|| format!("no such channel: {channel}"))?;
        let col = match col {
            Some(c) => {
                if c >= GRID_COLS {
                    return Err("slot out of range".to_string());
                }
                if ch.blocks.iter().any(|b| b.col == Some(c)) {
                    return Err("slot occupied".to_string());
                }
                c
            }
            None => {
                // If the channel is empty, start at col 0. Otherwise assign
                // max_col + 1 if that fits, else the lowest free slot.
                let max_col = ch.blocks.iter().filter_map(|b| b.col).max().unwrap_or(0);
                let col = if ch.blocks.is_empty() {
                    0
                } else if max_col + 1 < GRID_COLS {
                    max_col + 1
                } else {
                    let mut used = ch.blocks.iter().filter_map(|b| b.col).collect::<std::collections::HashSet<_>>();
                    let mut found = None;
                    for c in 0..GRID_COLS {
                        if !used.contains(&c) {
                            found = Some(c);
                            break;
                        }
                    }
                    found.unwrap_or(GRID_COLS)
                };
                if col >= GRID_COLS {
                    return Err("channel full (8 blocks)".to_string());
                }
                col
            }
        };
        let n = NodeDoc {
            id: id.clone(),
            block: block.into(),
            params: BTreeMap::new(),
            bypass: false,
            pos: Some(pos),
            model: None,
            curve: None,
            col: Some(col),
        };
        ch.blocks.push(n);
        ch.blocks.sort_by_key(|b| b.col.unwrap_or(0));
        Ok(id)
    }

    fn make_node(id: &str, block: &str, pos: [f32; 2]) -> Result<NodeDoc, String> {
        Ok(NodeDoc {
            id: id.to_string(),
            block: block.into(),
            params: BTreeMap::new(),
            bypass: false,
            pos: Some(pos),
            model: None,
            curve: None,
            col: None,
        })
    }

    /// Remove a block from a channel's strip.
    ///
    /// Every routing touching it — as source or as destination — goes with it.
    /// A routing is a connection between two existing blocks, so an endpoint
    /// that no longer exists cannot be honoured; leaving it in the list would
    /// draw a line to a block that is not there and give the audio path a
    /// dangling tap.
    pub fn remove_block(&mut self, channel: &str, id: &str) {
        if let Some(c) = self.channel_mut(channel) {
            c.blocks.retain(|b| b.id != id);
        }
        self.routings.retain(|r| {
            !(r.source.channel == channel && r.source.block == id)
                && !(r.dest.channel == channel && r.dest.block == id)
        });
    }

    /// Move a block within its strip. `to` is the target index.
    ///
    /// Reorders the block as today, then hands the channel's existing column
    /// values back out in ascending order (preserving gaps). Then prunes
    /// any routing that would violate the later-column rule.
    pub fn move_block(&mut self, channel: &str, id: &str, to: usize) {
        if let Some(c) = self.channel_mut(channel) {
            if let Some(from) = c.blocks.iter().position(|b| b.id == id) {
                let b = c.blocks.remove(from);
                let to = to.min(c.blocks.len());
                c.blocks.insert(to, b);
            }
        }
        self.reorder_cols_for_channel(channel);
        self.prune_invalid_routings(channel);
    }

    /// Reorder the columns of one channel's blocks back to ascending order,
    /// preserving any gaps.
    fn reorder_cols_for_channel(&mut self, channel: &str) {
        if let Some(ch) = self.channel_mut(channel) {
            let cols: std::collections::BTreeSet<u8> =
                ch.blocks.iter().filter_map(|b| b.col).collect();
            let mut sorted: Vec<u8> = cols.into_iter().collect();
            sorted.sort();
            let mut col_idx = 0;
            for b in &mut ch.blocks {
                b.col = Some(sorted[col_idx]);
                col_idx += 1;
            }
        }
    }

    // --- cross-channel routings -------------------------------------------

    /// Create a routing from `source` to `dest` and return its id.
    ///
    /// The source block keeps its own downstream connection; this only adds a
    /// feed into `dest`. Invalid routings are refused: unknown endpoints, a
    /// block routing to itself, a routing within one channel, an exact
    /// duplicate, and anything that would close a feedback loop.
    pub fn create_routing(
        &mut self,
        source: &RoutingEndpoint,
        dest: &RoutingEndpoint,
    ) -> Result<String, String> {
        self.validate_routing(source, dest)?;
        let id = self.fresh_routing_id();
        self.routings.push(Routing {
            id: id.clone(),
            source: source.clone(),
            dest: dest.clone(),
            enabled: true,
            gain: 1.0,
        });
        Ok(id)
    }

    /// Remove a routing by id. Returns whether it existed.
    pub fn remove_routing(&mut self, id: &str) -> bool {
        let before = self.routings.len();
        self.routings.retain(|r| r.id != id);
        self.routings.len() != before
    }

    /// Enable or disable a routing. Disabling keeps it in the list (and in the
    /// saved project) but takes its feed out of the signal path.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let r = self.routing(id).ok_or_else(|| format!("no such routing: {id}"))?;
        if enabled && !r.enabled && routing_creates_cycle(self, &r.source, &r.dest) {
            return Err("enabling that routing would create a feedback cycle".to_string());
        }
        self.routing_mut(id).expect("checked above").enabled = enabled;
        Ok(())
    }

    /// Set a routing's gain (linear). Optional in the API; the field defaults
    /// to 1.0 when a project does not carry it.
    pub fn set_gain(&mut self, id: &str, gain: f32) -> Result<(), String> {
        let r = self.routing_mut(id).ok_or_else(|| format!("no such routing: {id}"))?;
        r.gain = gain;
        Ok(())
    }

    /// Every routing, in creation order.
    pub fn list_routings(&self) -> &[Routing] {
        &self.routings
    }

    pub fn routing(&self, id: &str) -> Option<&Routing> {
        self.routings.iter().find(|r| r.id == id)
    }

    fn routing_mut(&mut self, id: &str) -> Option<&mut Routing> {
        self.routings.iter_mut().find(|r| r.id == id)
    }

    /// A fresh id that does not collide with an existing routing.
    fn fresh_routing_id(&self) -> String {
        let mut n = self.routings.len() + 1;
        while self.routing(&format!("route{n}")).is_some() {
            n += 1;
        }
        format!("route{n}")
    }

    /// Reject a routing that cannot be honoured. See [`Rig::create_routing`].
    pub fn validate_routing(
        &self,
        source: &RoutingEndpoint,
        dest: &RoutingEndpoint,
    ) -> Result<(), String> {
        self.check_endpoint("source", source)?;
        self.check_endpoint("destination", dest)?;
        if source.block == dest.block {
            return Err(format!("'{}' cannot route to itself", source.block));
        }
        if source.channel == dest.channel {
            return Err(format!(
                "a routing must cross channels: both ends are in '{}'",
                source.channel
            ));
        }
        if self.routings.iter().any(|r| &r.source == source && &r.dest == dest) {
            return Err("that routing already exists".to_string());
        }
        if routing_creates_cycle(self, source, dest) {
            return Err("that routing would create a feedback cycle".to_string());
        }
        // Split rule: dest col must be strictly later than source col.
        let source_col = self
            .channel(&source.channel)
            .and_then(|c| c.blocks.iter().find(|b| b.id == source.block))
            .and_then(|b| b.col);
        let dest_col = self
            .channel(&dest.channel)
            .and_then(|c| c.blocks.iter().find(|b| b.id == dest.block))
            .and_then(|b| b.col);
        if let (Some(s), Some(d)) = (source_col, dest_col) {
            if d <= s {
                return Err("a split must go to a later slot".to_string());
            }
        }
        Ok(())
    }

    fn check_endpoint(&self, role: &str, e: &RoutingEndpoint) -> Result<(), String> {
        let ch = self
            .channel(&e.channel)
            .ok_or_else(|| format!("{role}: no such channel: {}", e.channel))?;
        if !ch.blocks.iter().any(|b| b.id == e.block) {
            return Err(format!("{role}: no such block: {}", e.block));
        }
        Ok(())
    }

    /// Block-level signal edges: each channel's chain, the feeds a channel's
    /// sources give its head, and the enabled routings. `extra` adds a
    /// candidate edge (the routing being validated).
    fn signal_edges(&self, extra: Option<(&str, &str)>) -> Vec<(String, String)> {
        let mut edges: Vec<(String, String)> = Vec::new();
        for c in &self.channels {
            if c.blocks.is_empty() {
                continue;
            }
            for w in c.blocks.windows(2) {
                edges.push((w[0].id.clone(), w[1].id.clone()));
            }
            if let Some(last) = c.blocks.last() {
                edges.push((last.id.clone(), master_id(&c.id)));
            }
            let head = c.blocks[0].id.clone();
            for src in self.resolve_sources(&c.id) {
                if src != IN {
                    edges.push((master_id(&src), head.clone()));
                }
            }
        }
        for r in &self.routings {
            if r.enabled {
                edges.push((r.source.block.clone(), r.dest.block.clone()));
            }
        }
        if let Some((s, d)) = extra {
            edges.push((s.to_string(), d.to_string()));
        }
        edges
    }

    /// Apply the enabled routings to a compiled graph.
    ///
    /// This is where a routing becomes audio. It runs at compile time, so the
    /// saved model is never rewritten: the source block keeps its own
    /// downstream edge, and a `util.mix` is inserted in front of the
    /// destination block so the original driver and the tapped signal are
    /// summed. A routing with a gain other than unity gets a `util.gain` of its
    /// own in front of the mix.
    ///
    /// Chaining is automatic and order is [`Rig::routings`] order: a second
    /// routing into the same block finds the first mix as that block's current
    /// driver and nests, so the destination sums every feed. A disabled
    /// routing contributes nothing — no node, no edge — and the graph is
    /// byte-identical to one that never had it.
    ///
    /// See `docs/design/cross-channel-routing.md` §7.
    pub fn apply_routings(&self, graph: &mut Graph, reg: &Registry) -> Result<(), String> {
        for r in &self.routings {
            if !r.enabled {
                continue;
            }
            let dest = r.dest.block.as_str();
            // The destination's current driver is what the tap is added to, so
            // it has to be there: a block with no input cannot be summed into.
            let driver = graph
                .edges()
                .iter()
                .find(|e| e.to.0 == dest && e.to.1 == "in")
                .map(|e| e.from.clone())
                .ok_or_else(|| {
                    format!("routing '{}': '{dest}.in' has no input to mix with", r.id)
                })?;
            if graph.node(&r.source.block).is_none() {
                return Err(format!(
                    "routing '{}': no such node: {}",
                    r.id, r.source.block
                ));
            }

            let mix_id = format!("{ROUTING_MIX_PREFIX}{}", r.id);
            if graph.node(&mix_id).is_some() {
                return Err(format!(
                    "routing '{}': node id '{mix_id}' is already taken",
                    r.id
                ));
            }
            let mix = reg
                .build("util.mix")
                .ok_or_else(|| "routing: the registry has no 'util.mix' block".to_string())?;
            graph.add(mix_id.as_str(), mix).map_err(|e| e.to_string())?;
            graph
                .set_param_real(&mix_id, "a", 1.0)
                .map_err(|e| e.to_string())?;
            graph
                .set_param_real(&mix_id, "b", 1.0)
                .map_err(|e| e.to_string())?;

            // Only a non-unity routing carries a gain stage.
            let tap = if r.gain != 1.0 {
                let gain_id = format!("{ROUTING_GAIN_PREFIX}{}", r.id);
                if graph.node(&gain_id).is_some() {
                    return Err(format!(
                        "routing '{}': node id '{gain_id}' is already taken",
                        r.id
                    ));
                }
                let gain = reg
                    .build("util.gain")
                    .ok_or_else(|| "routing: the registry has no 'util.gain' block".to_string())?;
                graph
                    .add(gain_id.as_str(), gain)
                    .map_err(|e| e.to_string())?;
                // Linear gain → dB, clamped to what `util.gain` can express.
                let db = 20.0 * r.gain.clamp(1e-3, 15.85).log10();
                graph
                    .set_param_real(&gain_id, "gain", db)
                    .map_err(|e| e.to_string())?;
                Some(gain_id)
            } else {
                None
            };

            // Re-wire: the old driver and the tap meet at the mix, and the mix
            // alone drives the destination.
            graph.disconnect_input(dest, "in");
            graph
                .connect(&driver.0, &driver.1, &mix_id, "in_a")
                .map_err(|e| e.to_string())?;
            match &tap {
                Some(gain_id) => {
                    graph
                        .connect(&r.source.block, "out", gain_id, "in")
                        .map_err(|e| e.to_string())?;
                    graph
                        .connect(gain_id, "out", &mix_id, "in_b")
                        .map_err(|e| e.to_string())?;
                }
                None => {
                    graph
                        .connect(&r.source.block, "out", &mix_id, "in_b")
                        .map_err(|e| e.to_string())?;
                }
            }
            graph
                .connect(&mix_id, "out", dest, "in")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Move a block to a different channel at a given column.
    ///
    /// Returns `Err` if the channel/block don't exist, the target cell is
    /// occupied, or the channel doesn't exist. On success the block is moved
    /// (id, params, bypass, model unchanged), its routing endpoints are
    /// rewritten, and stale routings are pruned.
    pub fn place_block(
        &mut self,
        channel: &str,
        block: &str,
        to_channel: &str,
        col: u8,
    ) -> Result<(), String> {
        if col >= GRID_COLS {
            return Err("slot out of range".to_string());
        }
        // Validate source block exists.
        let src_ch = self
            .channel(channel)
            .ok_or_else(|| format!("no such channel: {channel}"))?;
        let src_block = src_ch
            .blocks
            .iter()
            .find(|b| b.id == block)
            .ok_or_else(|| format!("no such block: {block}"))?;
        // Validate target channel exists.
        let _tgt_ch = self
            .channel(to_channel)
            .ok_or_else(|| format!("no such channel: {to_channel}"))?;
        // No-op if same channel and same col (before checking occupied).
        if channel == to_channel && src_block.col == Some(col) {
            return Ok(());
        }
        // Check target cell is free.
        if let Some(tgt_ch) = self.channel(to_channel) {
            if tgt_ch.blocks.iter().any(|b| b.col == Some(col)) {
                return Err("slot occupied".to_string());
            }
        }
        // Clone src_block data before mutating channels.
        let src_data = src_block.clone();
        // Remove from source channel.
        if let Some(ch) = self.channel_mut(channel) {
            ch.blocks.retain(|b| b.id != block);
        }
        // Add to target channel at the given col.
        let mut new_block = src_data;
        new_block.col = Some(col);
        if let Some(ch) = self.channel_mut(to_channel) {
            ch.blocks.push(new_block);
            ch.blocks.sort_by_key(|b| b.col.unwrap_or(0));
        }
        // Rewrite routing endpoints.
        self.update_routing_endpoints(block, channel, to_channel);
        // Prune invalid routings.
        self.prune_invalid_routings(to_channel);
        Ok(())
    }

    /// Rewrite the channel field of every routing endpoint that names `block`.
    fn update_routing_endpoints(&mut self, block: &str, old_channel: &str, new_channel: &str) {
        for r in &mut self.routings {
            if r.source.block == block {
                r.source.channel = new_channel.to_string();
            }
            if r.dest.block == block {
                r.dest.channel = new_channel.to_string();
            }
        }
    }

    /// Prune routings that are now invalid (same-channel, self, same-col split, or cycles).
    fn prune_invalid_routings(&mut self, changed_channel: &str) {
        let old_len = self.routings.len();
        // Collect block-col pairs before mutable borrow of routings.
        let mut col_lookup: std::collections::HashMap<String, u8> =
            std::collections::HashMap::new();
        for ch in &self.channels {
            for b in &ch.blocks {
                if let Some(c) = b.col {
                    col_lookup.insert(b.id.clone(), c);
                }
            }
        }
        // Phase 1: cheap checks without borrowing self (retains the vector).
        self.routings.retain(|r| {
            // Same channel check.
            if r.source.channel == r.dest.channel {
                return false;
            }
            // Self check.
            if r.source.block == r.dest.block {
                return false;
            }
            // Split rule: dest col must be > source col.
            let (Some(s), Some(d)) = (
                col_lookup.get(&r.source.block).copied(),
                col_lookup.get(&r.dest.block).copied(),
            ) else {
                return true;
            };
            if d <= s {
                return false;
            }
            true
        });
        // Phase 2: cycle check — build a shallow copy of routings to pass to the checker.
        let shallow: Vec<(RoutingEndpoint, RoutingEndpoint)> =
            self.routings.iter().map(|r| (r.source.clone(), r.dest.clone())).collect();
        let mut to_remove = Vec::new();
        for (i, (src, dst)) in shallow.into_iter().enumerate() {
            if routing_creates_cycle(self, &src, &dst) {
                to_remove.push(i);
            }
        }
        for idx in to_remove.into_iter().rev() {
            self.routings.remove(idx);
        }
        // Keep the remaining routings in their original order.
        let new_len = self.routings.len();
        if new_len != old_len {
            // Reorder columns of the affected channels.
            self.reorder_cols_for_channel(changed_channel);
        }
    }

    // --- graph conversion -------------------------------------------------

    pub fn split_id(ch: &str) -> String {
        format!("{ch}__split")
    }
    /// The pedal's global output limiter. `volume` comes from `Rig::master_volume`
    /// (dB); the ceiling stays fixed at −1.0 dB.
    fn out_master(&self) -> NodeDoc {
        NodeDoc {
            id: OUT_MASTER_ID.into(),
            block: "util.master".into(),
            params: [
                ("volume".to_string(), self.master_volume),
                ("ceiling".to_string(), -1.0),
            ]
            .into_iter()
            .collect(),
            bypass: false,
            pos: Some([1320.0, 400.0]),
            model: None,
            curve: None,
            col: None,
        }
    }

    /// Lay the rig out as a graph the executor can compile.
    pub fn to_doc(&self) -> Doc {
        let mut doc = Doc::new(self.name.clone());
        // Routings are carried as their own list, not as edges: the compiled
        // graph is unchanged by a routing, so a save/load round trip cannot
        // silently rewire the source block.
        doc.routings = self.routings.clone();
        doc.nodes.push(NodeDoc {
            id: "in".into(),
            block: "io.input".into(),
            params: [("gain".to_string(), self.input_gain)]
                .into_iter()
                .collect(),
            bypass: false,
            pos: Some([60.0, 400.0]),
            model: None,
            curve: None,
            col: None,
        });
        doc.nodes.push(NodeDoc {
            id: "out".into(),
            block: "io.output".into(),
            params: BTreeMap::new(),
            bypass: false,
            pos: Some([1400.0, 400.0]),
            model: None,
            curve: None,
            col: None,
        });
        // A pedalboard with nothing in it is a *bypass*, not a dead pedal.
        // The summer below needs a channel master to feed it, so without this
        // the sum's inputs would all be tied to silence and the pedal would
        // output nothing until a block was added.
        let active: Vec<&Channel> = self
            .channels
            .iter()
            .filter(|c| !c.blocks.is_empty())
            .collect();
        if active.is_empty() {
            doc.nodes.push(self.out_master());
            doc.edges.push(EdgeDoc {
                from: ["in".to_string(), "out".into()],
                to: ["out__master".to_string(), "in".into()],
            });
            doc.edges.push(EdgeDoc {
                from: ["out__master".to_string(), "out".into()],
                to: ["out".to_string(), "in".into()],
            });
            return doc;
        }
        // One summing node so every channel master has exactly one wire into
        // the output node (which has a single input port).
        let mut sum = NodeDoc {
            id: "sum".into(),
            block: "util.sum".into(),
            params: BTreeMap::new(),
            bypass: false,
            pos: Some([1250.0, 400.0]),
            model: None,
            curve: None,
            col: None,
        };
        sum.params.insert("a".into(), 1.0);
        sum.params.insert("b".into(), 1.0);
        sum.params.insert("c".into(), 1.0);
        sum.params.insert("d".into(), 1.0);
        doc.nodes.push(sum);
        // Empty channels are skipped: they have no blocks, so they would only
        // duplicate the dry signal into the sum. Their slots stay in the model
        // and the UI; they just cost nothing until a block is dropped in.
        for (ci, c) in active.iter().enumerate() {
            let y = 240.0 + ci as f32 * 150.0;
            // An empty channel is transparent, so a channel fed by one is fed
            // by whatever feeds that one (see `resolve_sources`).
            let sources = self.resolve_sources(&c.id);
            let multi = sources.len() > 1;
            if multi {
                doc.nodes.push(NodeDoc {
                    id: Self::split_id(&c.id),
                    block: "util.mix".into(),
                    params: BTreeMap::new(),
                    bypass: false,
                    pos: Some([240.0, y]),
                    model: None,
                    curve: None,
                    col: None,
                });
            }
            for (i, b) in c.blocks.iter().enumerate() {
                let mut b = b.clone();
                b.pos = Some([400.0 + i as f32 * 175.0, y]);
                doc.nodes.push(b);
            }
            let mut master = NodeDoc {
                id: master_id(&c.id),
                block: "util.master".into(),
                params: BTreeMap::new(),
                bypass: false,
                pos: Some([400.0 + c.blocks.len() as f32 * 175.0, y]),
                model: None,
                curve: None,
                col: None,
            };
            master.params.insert("volume".into(), c.master_volume);
            master.params.insert("ceiling".into(), c.master_ceiling);
            doc.nodes.push(master);

            // The first node of the chain (the split, or the first block).
            let head = if multi {
                Self::split_id(&c.id)
            } else {
                c.blocks
                    .first()
                    .map(|b| b.id.clone())
                    .unwrap_or_else(|| master_id(&c.id))
            };
            // Routing edges into the head. (A channel's master is wired to
            // the pedal output by the channel-end edge below.)
            for (k, src) in sources.iter().enumerate() {
                let (from_node, from_port) = if src == IN {
                    ("in".to_string(), "out".to_string())
                } else {
                    (master_id(src), "out".to_string())
                };
                let to_port = if multi {
                    if k == 0 {
                        "in_a"
                    } else {
                        "in_b"
                    }
                } else {
                    "in"
                };
                doc.edges.push(EdgeDoc {
                    from: [from_node, from_port],
                    to: [head.clone(), to_port.into()],
                });
            }
            // The chain itself. With a split, the split feeds the first
            // block; without one, the first block *is* the head and the
            // routing edge above already feeds it.
            if multi {
                let mut prev = head.clone();
                for b in &c.blocks {
                    doc.edges.push(EdgeDoc {
                        from: [prev.clone(), "out".into()],
                        to: [b.id.clone(), "in".into()],
                    });
                    prev = b.id.clone();
                }
            } else {
                for w in c.blocks.windows(2) {
                    doc.edges.push(EdgeDoc {
                        from: [w[0].id.clone(), "out".into()],
                        to: [w[1].id.clone(), "in".into()],
                    });
                }
            }
            let last = if multi {
                head.clone()
            } else {
                c.blocks
                    .last()
                    .map(|b| b.id.clone())
                    .unwrap_or_else(|| head.clone())
            };
            // The last block feeds the channel master. (A channel always has
            // at least one block here, so `last` is never the master.)
            doc.edges.push(EdgeDoc {
                from: [last, "out".into()],
                to: [master_id(&c.id), "in".into()],
            });
            let port = ["a", "b", "c", "d"][ci.min(3)];
            doc.edges.push(EdgeDoc {
                from: [master_id(&c.id), "out".into()],
                to: ["sum".to_string(), port.into()],
            });
        }
        // Unused sum inputs are tied to silence via a zero-gain gain node, so
        // the graph's "every input driven" rule holds without a special case.
        for (k, port) in ["a", "b", "c", "d"].iter().enumerate().skip(active.len()) {
            let id = format!("sum_z{k}");
            doc.nodes.push(NodeDoc {
                id: id.clone(),
                block: "util.gain".into(),
                params: [("gain".to_string(), -60.0)].into_iter().collect(),
                bypass: false,
                pos: Some([1150.0, 200.0 + k as f32 * 60.0]),
                model: None,
                curve: None,
                col: None,
            });
            doc.edges.push(EdgeDoc {
                from: ["in".to_string(), "out".into()],
                to: [id.clone(), "in".into()],
            });
            doc.edges.push(EdgeDoc {
                from: [id, "out".into()],
                to: ["sum".to_string(), port.to_string()],
            });
        }
        // One limiter on the summed output: each channel has its own master,
        // but their sum can still exceed full scale.
        doc.nodes.push(self.out_master());
        doc.edges.push(EdgeDoc {
            from: ["sum".to_string(), "out".into()],
            to: ["out__master".to_string(), "in".into()],
        });
        doc.edges.push(EdgeDoc {
            from: ["out__master".to_string(), "out".into()],
            to: ["out".to_string(), "in".into()],
        });
        doc
    }

    /// Read a doc back into channels.
    ///
    /// A channel is a chain of blocks ending at a `util.master`. Walking
    /// backwards from the master, the chain stops at the first node whose
    /// input comes from the pedal input, from another channel's master, or
    /// from several sources at once (a summing point). That node is the
    /// channel's first block, and its inputs are the channel's sources.
    pub fn from_doc(doc: &Doc) -> Self {
        let mut rig = Rig {
            name: doc.name.clone(),
            channels: Vec::new(),
            routings: Vec::new(),
            input_gain: doc
                .node("in")
                .and_then(|n| n.params.get("gain"))
                .copied()
                .unwrap_or(0.0),
            master_volume: doc
                .node(OUT_MASTER_ID)
                .and_then(|n| n.params.get("volume"))
                .copied()
                .unwrap_or(0.0),
        };
        // Channel masters are the `util.master` nodes that are not the global
        // output limiter. Hand-written presets wire their masters straight to
        // `out` (there is no summer), while the app's own docs wire them to
        // `sum` and reserve `out__master` for the limiter — so the reserved id
        // is the one thing both shapes agree on.
        let masters: Vec<String> = {
            let mut v: Vec<String> = doc
                .nodes
                .iter()
                .filter(|n| n.block == "util.master" && n.id != OUT_MASTER_ID)
                .map(|n| n.id.clone())
                .collect();
            v.sort();
            v
        };
        for master in masters.iter() {
            let mut blocks: Vec<NodeDoc> = Vec::new();
            let mut cur = master.clone();
            let mut sources: Vec<String> = Vec::new();
            loop {
                let incoming: Vec<&EdgeDoc> = doc
                    .edges
                    .iter()
                    .filter(|e| e.to[0] == cur && e.to[1] == "in")
                    .collect();
                if incoming.is_empty() {
                    break;
                }
                if incoming.len() > 1 {
                    // A summing point: every input must be a channel source.
                    // The node itself belongs to this channel.
                    if let Some(n) = doc.node(&cur) {
                        if n.block != "util.master" {
                            blocks.push(n.clone());
                        }
                    }
                    sources = incoming
                        .iter()
                        .map(|e| src_name(&e.from[0]))
                        .collect();
                    break;
                }
                // `cur` is part of this channel's chain (it isn't the
                // master itself, which the loop's start handles).
                if let Some(n) = doc.node(&cur) {
                    if n.block != "util.master" {
                        blocks.push(n.clone());
                    }
                }
                let src = incoming[0].from[0].clone();
                if src == "in" || masters.contains(&src) || src.ends_with("__split") {
                    sources.push(src_name(&src));
                    break;
                }
                cur = src;
            }
            if sources.is_empty() {
                sources.push(IN.into());
            }
            let (master_volume, master_ceiling) = doc
                .node(master)
                .map(|n| {
                    (
                        n.params.get("volume").copied().unwrap_or(0.0),
                        n.params.get("ceiling").copied().unwrap_or(-1.0),
                    )
                })
                .unwrap_or((0.0, -1.0));
            blocks.reverse();
            // The channel id is the master's prefix, so a source name in the
            // preset ("amp") matches the channel that owns "amp__master".
            let id = master.strip_suffix("__master").unwrap_or(master).to_string();
            let n = rig.channels.len() + 1;
            rig.channels
                .push(Channel {
                    id,
                    name: format!("Channel {n}"),
                    blocks,
                    sources,
                    master_volume,
                    master_ceiling,
                });
        }
        // A rig always shows the full set of channel slots. A preset with
        // fewer channels (every shipped one has one) gets empty slots rather
        // than a UI that grows and shrinks.
        rig.pad_channels();
        // Normalize columns on every block in every loaded channel.
        for ch in &mut rig.channels {
            if let Err(e) = ch.normalize_cols() {
                eprintln!("pedal: loading preset for rig '{}': {e}", rig.name);
                // Continue anyway — a channel with too many blocks is still
                // loadable without columns.
            }
        }
        // Routings are validated on load too: a hand-edited project must not be
        // able to smuggle in a self-route, a same-channel route, a duplicate,
        // or a cycle. Invalid entries are dropped rather than poisoning the rig.
        for r in &doc.routings {
            if let Err(e) = rig.validate_routing(&r.source, &r.dest) {
                eprintln!("pedal: dropping routing '{}' on load: {e}", r.id);
                continue;
            }
            rig.routings.push(r.clone());
        }
        rig
    }

    /// Fill the channel slots up to [`CHANNEL_COUNT`] with empty channels and
    /// drop any beyond it, so a rig always has the same shape.
    fn pad_channels(&mut self) {
        self.channels.truncate(CHANNEL_COUNT);
        while self.channels.len() < CHANNEL_COUNT {
            let n = self.channels.len() + 1;
            let mut id = format!("ch{n}");
            let mut k = 2;
            while self
                .channels
                .iter()
                .any(|c| c.id == id)
            {
                id = format!("ch{n}_{k}");
                k += 1;
            }
            self.channels
                .push(Channel::new(&id, &format!("Channel {n}")));
        }
    }

    /// The sources that actually feed a channel once empty channels are
    /// skipped, deduplicated. An empty channel is transparent: it passes its
    /// own sources through, so a channel fed by an empty one is fed by
    /// whatever feeds that one.
    fn resolve_sources(&self, id: &str) -> Vec<String> {
        let Some(ch) = self.channel(id) else {
            return vec![IN.to_string()];
        };
        let mut seen = vec![id.to_string()];
        let mut out = Vec::new();
        for s in &ch.sources {
            if s == IN {
                out.push(IN.to_string());
            } else {
                out.extend(self.resolve_source(s, &mut seen));
            }
        }
        if out.is_empty() {
            out.push(IN.to_string());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Resolve one source name. An active channel stands for its own master;
    /// an empty one is transparent, so its sources are used instead. A routing
    /// cycle falls back to the pedal input rather than recursing forever.
    fn resolve_source(&self, id: &str, seen: &mut Vec<String>) -> Vec<String> {
        if seen.iter().any(|s| s == id) {
            return vec![IN.to_string()];
        }
        let Some(ch) = self.channel(id) else {
            return vec![IN.to_string()];
        };
        if !ch.blocks.is_empty() {
            return vec![id.to_string()];
        }
        seen.push(id.to_string());
        let mut out = Vec::new();
        for s in &ch.sources {
            if s == IN {
                out.push(IN.to_string());
            } else {
                out.extend(self.resolve_source(s, seen));
            }
        }
        seen.pop();
        if out.is_empty() {
            out.push(IN.to_string());
        }
        out
    }
}

fn src_name(node: &str) -> String {
    if node == "in" {
        IN.into()
    } else if let Some(s) = node.strip_suffix("__master") {
        s.into()
    } else {
        node.into()
    }
}
