//! From a description to a graph the live threads can run: checked,
//! ordered, every node given to one thread, every output port given a
//! buffer.
//!
//! **Which thread runs which node is decided here, once** (decision
//! D-10, 25 November 2026): a node runs on the same live CPU in every
//! cycle. The cycle is the same from one period to the next, the data of
//! a node stay in the cache of its CPU, and in Phase 4 the host of a
//! plug-in knows on which CPU it is waited for.
//!
//! The assignment is a schedule made on paper. Every node has an
//! expected cost ([`Costs`]); the nodes are taken from the one with the
//! longest chain of work still behind it to the one with the shortest,
//! which is also an order in which every node comes after what it
//! depends on; each is given to the thread on which it would end
//! soonest, counting when its inputs are ready, when the thread is free,
//! and a loss when an input comes from another thread. The nodes of the
//! sound card go to thread 0, which holds the card.
//!
//! Two things keep a signal from changing CPU for nothing. A chain of
//! nodes that only wait for each other is a **segment**, given to a
//! thread whole. And the schedule is made for one thread, for two, up to
//! all of them: the shortest is kept, on the fewest threads that give
//! it.
//!
//! What comes out is as good as the costs it started from: they are
//! estimates until step P1.7c measures them.

use std::fmt::Write as _;

use crate::kind::{Costs, Inputs, Kind};
use crate::text::Description;
use crate::Error;

/// What a graph is compiled for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Live threads that may be given nodes.
    pub threads: usize,
    /// Sample rate in hertz.
    pub rate: u32,
    /// Frames in a period: the quantum.
    pub frames: u32,
    /// What each kind of node is expected to cost.
    pub costs: Costs,
}

/// A node of a compiled graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Its name in the description.
    pub name: String,
    /// What it does.
    pub kind: Kind,
    /// The line of the description that declares it.
    pub line: usize,
    /// The live thread that runs it.
    pub thread: usize,
    /// The buffer each input reads; `None`: nothing connected, silence.
    /// For a sum and a bus, one per connection.
    pub inputs: Vec<Option<usize>>,
    /// The buffer each output writes.
    pub outputs: Vec<usize>,
    /// The nodes it takes an input from, as places in
    /// [`Compiled::nodes`]: each once, all before it.
    pub waits_for: Vec<usize>,
    /// The nodes that take an input from it: each once, all after it.
    pub feeds: Vec<usize>,
    /// Nanoseconds it is expected to take.
    pub cost_ns: u64,
    /// Nanoseconds from the start of the cycle at which it is expected
    /// to start.
    pub start_ns: u64,
}

/// A graph ready to be run.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// The nodes, in an order in which each comes after the nodes it
    /// waits for.
    pub nodes: Vec<Node>,
    /// For each live thread, its nodes in the order it runs them, as
    /// places in [`Compiled::nodes`].
    pub threads: Vec<Vec<usize>>,
    /// How many buffers of [`Compiled::frames`] frames the outputs of the
    /// nodes need.
    pub buffers: usize,
    /// Sample rate in hertz.
    pub rate: u32,
    /// Frames in a period.
    pub frames: u32,
    /// Nanoseconds all the nodes are expected to take, one after the
    /// other.
    pub work_ns: u64,
    /// Nanoseconds of the longest chain of nodes that wait for each
    /// other: no number of threads makes the cycle shorter than this.
    pub critical_ns: u64,
    /// The nodes of that chain, as places in [`Compiled::nodes`].
    pub critical_path: Vec<usize>,
    /// Nanoseconds from the start of the cycle to the end of the last
    /// node, as the threads are expected to run them.
    pub makespan_ns: u64,
    /// What is odd without being wrong.
    pub warnings: Vec<String>,
    /// The costs the graph was compiled with: they also say what every
    /// load is made of.
    pub costs: Costs,
}

/// Where an input comes from: a node of the description, one of its
/// output ports.
type Source = (usize, u32);

/// Compile `description` for `settings`.
pub fn compile(description: &Description, settings: &Settings) -> Result<Compiled, Error> {
    let declared = &description.nodes;
    let count = declared.len();
    if count == 0 {
        return Err(Error::new(0, "the graph has no node"));
    }
    if settings.threads == 0 || settings.frames == 0 || settings.rate == 0 {
        return Err(Error::new(
            0,
            "a graph is compiled for at least one thread, one frame and one hertz",
        ));
    }

    // ---- The thread a node must be on, when it has no choice.
    let mut pinned = Vec::with_capacity(count);
    for node in declared {
        if let Some(thread) = node.thread {
            if thread >= settings.threads {
                return Err(Error::new(
                    node.line,
                    format!(
                        "thread={thread}: there are {} live threads, from 0",
                        settings.threads
                    ),
                ));
            }
            if node.kind.on_the_card() && thread != 0 {
                return Err(Error::new(
                    node.line,
                    format!(
                        "'{}' is an {} of the sound card: it runs on thread 0, which holds the card",
                        node.name,
                        node.kind.word()
                    ),
                ));
            }
        }
        pinned.push(if node.kind.on_the_card() {
            Some(0)
        } else {
            node.thread
        });
    }

    // ---- Two outputs on one channel of the card would overwrite each
    // other.
    for (place, node) in declared.iter().enumerate() {
        if let Kind::Output { channel } = node.kind {
            let earlier = declared[..place]
                .iter()
                .find(|other| other.kind == Kind::Output { channel });
            if let Some(earlier) = earlier {
                return Err(Error::new(
                    node.line,
                    format!(
                        "channel {channel} of the card is already the output '{}' (line {}): \
                         mix the two with a sum",
                        earlier.name, earlier.line
                    ),
                ));
            }
        }
    }

    // ---- The connections, port by port.
    let mut fixed: Vec<Vec<Option<(Source, usize)>>> = declared
        .iter()
        .map(|node| match node.kind.inputs() {
            Inputs::Fixed(ports) => vec![None; ports as usize],
            Inputs::Any => Vec::new(),
        })
        .collect();
    let mut any: Vec<Vec<Source>> = vec![Vec::new(); count];
    let mut taken: Vec<Vec<bool>> = declared
        .iter()
        .map(|node| vec![false; node.kind.outputs() as usize])
        .collect();
    for link in &description.links {
        let from = &declared[link.from.node];
        let to = &declared[link.to.node];
        let outputs = from.kind.outputs();
        if outputs == 0 {
            return Err(Error::new(
                link.line,
                format!(
                    "'{}' is a {}: it has no output",
                    from.name,
                    from.kind.word()
                ),
            ));
        }
        let out_port = link.from.port.unwrap_or(0);
        if out_port >= outputs {
            return Err(Error::new(
                link.line,
                format!(
                    "'{}' has {outputs} output{}, from 0: there is no output {out_port}",
                    from.name,
                    if outputs == 1 { "" } else { "s" }
                ),
            ));
        }
        let source = (link.from.node, out_port);
        match to.kind.inputs() {
            Inputs::Fixed(0) => {
                return Err(Error::new(
                    link.line,
                    format!("'{}' is a {}: it has no input", to.name, to.kind.word()),
                ));
            }
            Inputs::Fixed(ports) => {
                let in_port = link.to.port.unwrap_or(0);
                if in_port >= ports {
                    return Err(Error::new(
                        link.line,
                        format!(
                            "'{}' has {ports} input{}, from 0: there is no input {in_port}",
                            to.name,
                            if ports == 1 { "" } else { "s" }
                        ),
                    ));
                }
                let slot = &mut fixed[link.to.node][in_port as usize];
                if let Some((_, line)) = slot {
                    return Err(Error::new(
                        link.line,
                        format!(
                            "input {in_port} of '{}' is already connected on line {line}: \
                             to mix two signals use a sum",
                            to.name
                        ),
                    ));
                }
                *slot = Some((source, link.line));
            }
            Inputs::Any => {
                if link.to.port.is_some() {
                    return Err(Error::new(
                        link.line,
                        format!(
                            "'{}' is a {}: its inputs have no number",
                            to.name,
                            to.kind.word()
                        ),
                    ));
                }
                any[link.to.node].push(source);
            }
        }
        taken[link.from.node][out_port as usize] = true;
    }

    // ---- Who waits for whom.
    let sources_of = |node: usize| -> Vec<Source> {
        fixed[node]
            .iter()
            .flatten()
            .map(|(source, _)| *source)
            .chain(any[node].iter().copied())
            .collect()
    };
    let mut waits_for: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut feeds: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (node, waits) in waits_for.iter_mut().enumerate() {
        for (source, _) in sources_of(node) {
            if !waits.contains(&source) {
                waits.push(source);
                feeds[source].push(node);
            }
        }
    }

    // ---- No circle: take away, again and again, the nodes that wait
    // for nothing still there. What is left waits in a circle.
    let mut waiting: Vec<usize> = waits_for.iter().map(Vec::len).collect();
    let mut ready: Vec<usize> = (0..count).filter(|node| waiting[*node] == 0).collect();
    let mut sorted = Vec::with_capacity(count);
    while let Some(node) = ready.pop() {
        sorted.push(node);
        for &next in &feeds[node] {
            waiting[next] -= 1;
            if waiting[next] == 0 {
                ready.push(next);
            }
        }
    }
    if sorted.len() < count {
        let left: Vec<&str> = (0..count)
            .filter(|node| waiting[*node] > 0)
            .map(|node| declared[node].name.as_str())
            .collect();
        let first = (0..count).find(|node| waiting[*node] > 0).unwrap_or(0);
        return Err(Error::new(
            declared[first].line,
            format!(
                "these nodes wait for each other in a circle, or for nodes that do: {}. \
                 A graph has no way back",
                left.join(", ")
            ),
        ));
    }

    // ---- Segments: a node that waits for one node alone, which feeds
    // it alone, goes wherever that node goes. Chains like that are
    // given to a thread whole (decision D-10 speaks of segments): a
    // signal does not change CPU where nothing is gained by it.
    let mut segment_of = vec![usize::MAX; count];
    let mut segments: Vec<Vec<usize>> = Vec::new();
    for &node in &sorted {
        let joined = match waits_for[node].as_slice() {
            [before] if feeds[*before].len() == 1 && pinned[*before] == pinned[node] => {
                Some(segment_of[*before])
            }
            _ => None,
        };
        let segment = joined.unwrap_or_else(|| {
            segments.push(Vec::new());
            segments.len() - 1
        });
        segments[segment].push(node);
        segment_of[node] = segment;
    }

    // ---- What each node costs, and for each segment how much work
    // there still is from its start to the end of the longest chain it
    // is on.
    let cost: Vec<u64> = (0..count)
        .map(|node| {
            settings.costs.of(
                &declared[node].kind,
                sources_of(node).len(),
                settings.frames,
            )
        })
        .collect();
    let segment_cost: Vec<u64> = segments
        .iter()
        .map(|nodes| nodes.iter().map(|node| cost[*node]).sum())
        .collect();
    // Only the first node of a segment waits for other segments, and
    // only the last feeds them.
    let segment_waits: Vec<Vec<usize>> = segments
        .iter()
        .map(|nodes| {
            waits_for[nodes[0]]
                .iter()
                .map(|node| segment_of[*node])
                .collect()
        })
        .collect();
    let segment_feeds: Vec<Vec<usize>> = segments
        .iter()
        .map(|nodes| {
            feeds[nodes[nodes.len() - 1]]
                .iter()
                .map(|node| segment_of[*node])
                .collect()
        })
        .collect();
    // The segments were made in an order in which each comes after what
    // it waits for: backwards, what a segment feeds is ranked first.
    let mut rank = vec![0_u64; segments.len()];
    for segment in (0..segments.len()).rev() {
        let behind = segment_feeds[segment]
            .iter()
            .map(|next| rank[*next])
            .max()
            .unwrap_or(0);
        rank[segment] = segment_cost[segment] + behind;
    }

    // ---- The order: the longest chain behind first. A node costs at
    // least a nanosecond, so whoever waits comes after.
    let mut segment_order: Vec<usize> = (0..segments.len()).collect();
    segment_order.sort_by(|a, b| {
        rank[*b]
            .cmp(&rank[*a])
            .then(segments[*a][0].cmp(&segments[*b][0]))
    });

    // ---- The schedule on paper.
    // The loss is a small positive number: nothing is lost by rounding.
    let other_thread_ns = settings.costs.other_thread_ns.round().max(0.0) as u64;
    // The same schedule is made for one thread, for two, and so on up to
    // all of them, and the one that ends soonest is kept; of two that
    // end together, the one on fewer threads. A graph of light nodes is
    // not spread over the CPUs for the sake of it: crossing costs.
    let schedule = |limit: usize| {
        let mut free = vec![0_u64; settings.threads];
        let mut segment_thread = vec![0_usize; segments.len()];
        let mut segment_end = vec![0_u64; segments.len()];
        let mut thread_of = vec![0_usize; count];
        let mut start = vec![0_u64; count];
        let mut end = vec![0_u64; count];
        for &segment in &segment_order {
            let candidates = match pinned[segments[segment][0]] {
                Some(thread) => thread..thread + 1,
                None => 0..limit,
            };
            // The thread of the input that is ready last: where the segment
            // goes when two threads would do as well.
            let last_input = segment_waits[segment]
                .iter()
                .max_by_key(|source| segment_end[**source])
                .map(|source| segment_thread[*source]);
            let mut best: Option<(u64, usize, u64)> = None;
            for thread in candidates {
                let inputs_ready = segment_waits[segment]
                    .iter()
                    .map(|source| {
                        segment_end[*source]
                            + if segment_thread[*source] == thread {
                                0
                            } else {
                                other_thread_ns
                            }
                    })
                    .max()
                    .unwrap_or(0);
                let starts = inputs_ready.max(free[thread]);
                let ends = starts + segment_cost[segment];
                let better = match best {
                    None => true,
                    Some((best_end, best_thread, _)) => {
                        ends < best_end
                            || (ends == best_end
                                && Some(thread) == last_input
                                && Some(best_thread) != last_input)
                    }
                };
                if better {
                    best = Some((ends, thread, starts));
                }
            }
            // There is always a candidate: one thread at the least.
            let (ends, thread, starts) = best.unwrap_or((segment_cost[segment], 0, 0));
            segment_thread[segment] = thread;
            segment_end[segment] = ends;
            free[thread] = ends;
            let mut at = starts;
            for &node in &segments[segment] {
                thread_of[node] = thread;
                start[node] = at;
                at += cost[node];
                end[node] = at;
            }
        }
        let makespan = end.iter().copied().max().unwrap_or(0);
        (makespan, thread_of, start, end)
    };
    let mut chosen = schedule(1);
    for limit in 2..=settings.threads {
        let other = schedule(limit);
        if other.0 < chosen.0 {
            chosen = other;
        }
    }
    let (_, thread_of, start, end) = chosen;
    // The nodes, segment after segment.
    let order: Vec<usize> = segment_order
        .iter()
        .flat_map(|segment| segments[*segment].iter().copied())
        .collect();

    // ---- Buffers, and the nodes in their new places.
    let mut place = vec![0_usize; count];
    for (new, &old) in order.iter().enumerate() {
        place[old] = new;
    }
    let mut first_buffer = vec![0_usize; count];
    let mut buffers = 0_usize;
    for &old in &order {
        first_buffer[old] = buffers;
        buffers += declared[old].kind.outputs() as usize;
    }
    let buffer_of = |(node, port): Source| first_buffer[node] + port as usize;

    let mut warnings = Vec::new();
    let mut nodes = Vec::with_capacity(count);
    let mut threads = vec![Vec::new(); settings.threads];
    for (new, &old) in order.iter().enumerate() {
        let node = &declared[old];
        let inputs: Vec<Option<usize>> = match node.kind.inputs() {
            Inputs::Fixed(_) => fixed[old]
                .iter()
                .map(|slot| slot.map(|(source, _)| buffer_of(source)))
                .collect(),
            Inputs::Any => any[old]
                .iter()
                .map(|source| Some(buffer_of(*source)))
                .collect(),
        };
        for (port, input) in inputs.iter().enumerate() {
            if input.is_none() {
                warnings.push(format!(
                    "'{}' (line {}): nothing is connected to its input {port}: it gets silence",
                    node.name, node.line
                ));
            }
        }
        if node.kind.inputs() == Inputs::Any && inputs.is_empty() {
            warnings.push(format!(
                "'{}' (line {}): nothing is connected to this {}: it gives silence",
                node.name,
                node.line,
                node.kind.word()
            ));
        }
        if !taken[old].is_empty() && taken[old].iter().all(|taken| !taken) {
            warnings.push(format!(
                "'{}' (line {}): nothing takes what it makes: it is computed for nothing",
                node.name, node.line
            ));
        }
        let mut before: Vec<usize> = waits_for[old].iter().map(|other| place[*other]).collect();
        before.sort_unstable();
        let mut after: Vec<usize> = feeds[old].iter().map(|other| place[*other]).collect();
        after.sort_unstable();
        threads[thread_of[old]].push(new);
        nodes.push(Node {
            name: node.name.clone(),
            kind: node.kind,
            line: node.line,
            thread: thread_of[old],
            inputs,
            outputs: (0..node.kind.outputs() as usize)
                .map(|port| first_buffer[old] + port)
                .collect(),
            waits_for: before,
            feeds: after,
            cost_ns: cost[old],
            start_ns: start[old],
        });
    }

    // ---- The longest chain, node by node.
    let mut critical_path = Vec::new();
    let mut at = segment_order.first().copied();
    while let Some(segment) = at {
        critical_path.extend(segments[segment].iter().map(|node| place[*node]));
        at = segment_feeds[segment].iter().copied().max_by(|a, b| {
            // The longest chain behind; of two as long, the one declared
            // first.
            rank[*a]
                .cmp(&rank[*b])
                .then(segments[*b][0].cmp(&segments[*a][0]))
        });
    }

    Ok(Compiled {
        nodes,
        threads,
        buffers,
        rate: settings.rate,
        frames: settings.frames,
        work_ns: cost.iter().sum(),
        critical_ns: segment_order.first().map_or(0, |segment| rank[*segment]),
        critical_path,
        makespan_ns: end.iter().copied().max().unwrap_or(0),
        warnings,
        costs: settings.costs,
    })
}

/// Nanoseconds as microseconds with two decimals.
fn us(ns: u64) -> String {
    format!("{}.{:02}", ns / 1000, (ns % 1000) / 10)
}

impl Compiled {
    /// Nanoseconds in a period.
    #[must_use]
    pub fn period_ns(&self) -> u64 {
        u64::from(self.frames) * 1_000_000_000 / u64::from(self.rate)
    }

    /// Nanoseconds thread `thread` is expected to work in a cycle.
    #[must_use]
    pub fn thread_work_ns(&self, thread: usize) -> u64 {
        self.threads[thread]
            .iter()
            .map(|node| self.nodes[*node].cost_ns)
            .sum()
    }

    /// What was compiled, for a person: what the graph is expected to
    /// cost, whether it fits in the compute budget (a period less
    /// `margin_us` microseconds), and what every thread runs, in order.
    #[must_use]
    pub fn summary(&self, margin_us: u64) -> String {
        let mut text = String::new();
        let period = self.period_ns();
        let budget = period.saturating_sub(margin_us * 1000);
        // Writing to a String cannot fail.
        let _ = self.write_summary(&mut text, period, budget, margin_us);
        text
    }

    fn write_summary(
        &self,
        text: &mut String,
        period: u64,
        budget: u64,
        margin_us: u64,
    ) -> std::fmt::Result {
        let mut kinds: Vec<(&str, usize)> = Vec::new();
        for node in &self.nodes {
            match kinds.iter_mut().find(|(word, _)| *word == node.kind.word()) {
                Some((_, count)) => *count += 1,
                None => kinds.push((node.kind.word(), 1)),
            }
        }
        let kinds: Vec<String> = kinds
            .iter()
            .map(|(word, count)| format!("{count} {word}"))
            .collect();
        let used = self
            .threads
            .iter()
            .filter(|nodes| !nodes.is_empty())
            .count();
        writeln!(text, "graph_rate_hz: {}", self.rate)?;
        writeln!(
            text,
            "graph_quantum_frames: {} (a period of {} us)",
            self.frames,
            us(period)
        )?;
        writeln!(
            text,
            "graph_nodes: {} ({})",
            self.nodes.len(),
            kinds.join(", ")
        )?;
        writeln!(
            text,
            "graph_buffers: {} of {} frames",
            self.buffers, self.frames
        )?;
        writeln!(
            text,
            "graph_threads: {}, {} of them with nodes",
            self.threads.len(),
            used
        )?;
        writeln!(
            text,
            "graph_costs: expected; a load is made of {} ns per frame, {} for every stage and {} for every trim",
            self.costs.load_frame_ns, self.costs.stage_ns, self.costs.trim_ns
        )?;
        // What each load of the graph is made of: the calibration of
        // ADR 0011, to be read in every report.
        let mut loads: Vec<u32> = self
            .nodes
            .iter()
            .filter_map(|node| match node.kind {
                Kind::Load { ns_per_frame } => Some(ns_per_frame),
                _ => None,
            })
            .collect();
        loads.sort_unstable();
        loads.dedup();
        for ns in loads {
            let (stages, trims) = self.costs.load_shape(ns);
            writeln!(
                text,
                "graph_load_of_{ns}_ns: {stages} stages and {trims} trims, expected {:.2} ns per frame",
                self.costs.cascade_ns(stages, trims)
            )?;
        }
        writeln!(
            text,
            "graph_work_us: {} (every node, one after the other)",
            us(self.work_ns)
        )?;
        let path: Vec<&str> = self
            .critical_path
            .iter()
            .map(|node| self.nodes[*node].name.as_str())
            .collect();
        writeln!(
            text,
            "graph_critical_path_us: {} ({})",
            us(self.critical_ns),
            path.join(" -> ")
        )?;
        let ratio = |work: u64, time: u64| {
            if time == 0 {
                "-".to_string()
            } else {
                format!("{}.{:02}", work / time, (work % time) * 100 / time)
            }
        };
        writeln!(
            text,
            "graph_makespan_us: {} on {} thread{} (speed-up {}; the critical path allows {} at the most)",
            us(self.makespan_ns),
            self.threads.len(),
            if self.threads.len() == 1 { "" } else { "s" },
            ratio(self.work_ns, self.makespan_ns),
            ratio(self.work_ns, self.critical_ns)
        )?;
        writeln!(
            text,
            "graph_budget_us: {} (a period less {margin_us} of margin): {}",
            us(budget),
            if self.makespan_ns <= budget {
                "the graph fits"
            } else {
                "THE GRAPH DOES NOT FIT"
            }
        )?;
        writeln!(text)?;
        writeln!(text, "threads (expected work in a cycle):")?;
        writeln!(
            text,
            "{:<7} {:>6} {:>10} {:>18}",
            "thread", "nodes", "work_us", "percent_of_period"
        )?;
        for (thread, nodes) in self.threads.iter().enumerate() {
            let work = self.thread_work_ns(thread);
            writeln!(
                text,
                "{:<7} {:>6} {:>10} {:>18}",
                thread,
                nodes.len(),
                us(work),
                format!("{}.{}", work * 100 / period, (work * 1000 / period) % 10)
            )?;
        }
        for (thread, nodes) in self.threads.iter().enumerate() {
            if nodes.is_empty() {
                continue;
            }
            writeln!(text)?;
            writeln!(
                text,
                "thread {thread}, in the order it runs them (expected microseconds from the start of the cycle):"
            )?;
            writeln!(
                text,
                "{:>9} {:>9}  {:<20} {:<8} waits for",
                "start_us", "cost_us", "node", "kind"
            )?;
            for &place in nodes {
                let node = &self.nodes[place];
                let waits: Vec<String> = node
                    .waits_for
                    .iter()
                    .map(|other| {
                        let other = &self.nodes[*other];
                        if other.thread == thread {
                            other.name.clone()
                        } else {
                            format!("{} (thread {})", other.name, other.thread)
                        }
                    })
                    .collect();
                writeln!(
                    text,
                    "{:>9} {:>9}  {:<20} {:<8} {}",
                    us(node.start_ns),
                    us(node.cost_ns),
                    node.name,
                    node.kind.word(),
                    if waits.is_empty() {
                        "-".to_string()
                    } else {
                        waits.join(", ")
                    }
                )?;
            }
        }
        if !self.warnings.is_empty() {
            writeln!(text)?;
            writeln!(text, "warnings:")?;
            for warning in &self.warnings {
                writeln!(text, "  {warning}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::parse;

    fn settings(threads: usize) -> Settings {
        Settings {
            threads,
            rate: 48_000,
            frames: 96,
            costs: Costs::default(),
        }
    }

    fn compiled(text: &str, threads: usize) -> Compiled {
        compile(&parse(text).unwrap(), &settings(threads)).unwrap()
    }

    fn place(graph: &Compiled, name: &str) -> usize {
        graph
            .nodes
            .iter()
            .position(|node| node.name == name)
            .unwrap()
    }

    /// What must hold of every compiled graph.
    fn check(graph: &Compiled) {
        let mut seen = vec![false; graph.nodes.len()];
        for (thread, nodes) in graph.threads.iter().enumerate() {
            let mut last = None;
            for &node in nodes {
                assert!(!seen[node], "a node is on two threads");
                seen[node] = true;
                assert_eq!(graph.nodes[node].thread, thread);
                assert!(last < Some(node), "a thread runs its nodes out of order");
                last = Some(node);
            }
        }
        assert!(seen.iter().all(|seen| *seen), "a node is on no thread");
        let mut written = vec![false; graph.buffers];
        for (place, node) in graph.nodes.iter().enumerate() {
            for &other in &node.waits_for {
                assert!(other < place, "a node comes before what it waits for");
                assert!(graph.nodes[other].feeds.contains(&place));
                let ready = graph.nodes[other].start_ns + graph.nodes[other].cost_ns;
                assert!(
                    node.start_ns >= ready,
                    "a node starts before its input is ready"
                );
            }
            for &buffer in node.inputs.iter().flatten() {
                assert!(written[buffer], "a node reads a buffer nobody wrote yet");
            }
            for &buffer in &node.outputs {
                assert!(!written[buffer], "two outputs write one buffer");
                written[buffer] = true;
            }
            if node.kind.on_the_card() {
                assert_eq!(node.thread, 0);
            }
        }
        assert!(graph.critical_ns <= graph.makespan_ns);
        assert!(graph.makespan_ns <= graph.work_ns + 1000 * graph.nodes.len() as u64);
    }

    #[test]
    fn a_chain_stays_on_one_thread() {
        let graph = compiled(
            "input in3 channel=3\n\
             gain soft db=-6\n\
             delay late frames=48\n\
             output out5 channel=5\n\
             connect in3 -> soft -> late -> out5\n",
            4,
        );
        check(&graph);
        assert!(graph.nodes.iter().all(|node| node.thread == 0));
        assert_eq!(graph.threads[0].len(), 4);
        assert_eq!(graph.buffers, 3);
        // One after the other: nothing to gain from more threads.
        assert_eq!(graph.makespan_ns, graph.work_ns);
        assert_eq!(graph.critical_ns, graph.work_ns);
        let names: Vec<&str> = graph
            .critical_path
            .iter()
            .map(|node| graph.nodes[*node].name.as_str())
            .collect();
        assert_eq!(names, ["in3", "soft", "late", "out5"]);
        assert!(graph.warnings.is_empty());
    }

    #[test]
    fn four_loads_side_by_side_take_four_threads() {
        let mut text = String::new();
        for index in 0..4 {
            text.push_str(&format!(
                "noise n{index} seed={index}\nload l{index} class=heavy\npeak p{index}\n\
                 connect n{index} -> l{index}:0 -> p{index}\nconnect n{index} -> l{index}:1\n"
            ));
        }
        let one = compiled(&text, 1);
        let four = compiled(&text, 4);
        check(&one);
        check(&four);
        assert_eq!(one.makespan_ns, one.work_ns);
        for thread in 0..4 {
            assert_eq!(four.threads[thread].len(), 3, "thread {thread}");
        }
        // Four times as fast, and a chain is a quarter of the work.
        assert_eq!(four.makespan_ns * 4, four.work_ns);
        assert_eq!(four.critical_ns * 4, four.work_ns);
    }

    #[test]
    fn a_track_is_given_to_a_thread_whole() {
        // Six tracks of three loads each, of different weights: every
        // load of a track waits for the one before it alone.
        let mut text = String::from("bus mix\npeak p\nconnect mix -> p\n");
        for track in 0..6 {
            let class = ["light", "medium", "heavy"][track % 3];
            text.push_str(&format!("noise t{track} seed={track}\n"));
            let mut before = format!("t{track}");
            for slot in 0..3 {
                text.push_str(&format!(
                    "load t{track}.{slot} class={class}\nconnect {before} -> t{track}.{slot}:0\n"
                ));
                before = format!("t{track}.{slot}");
            }
            text.push_str(&format!("connect {before} -> mix\n"));
        }
        let graph = compiled(&text, 4);
        check(&graph);
        for track in 0..6 {
            let thread = graph.nodes[place(&graph, &format!("t{track}"))].thread;
            for slot in 0..3 {
                let node = &graph.nodes[place(&graph, &format!("t{track}.{slot}"))];
                assert_eq!(node.thread, thread, "track {track}");
            }
        }
        // More than one thread is worth it here.
        assert!(graph.makespan_ns * 2 < graph.work_ns);
    }

    #[test]
    fn light_nodes_are_not_spread_for_the_sake_of_it() {
        // Eight inputs of the card, a gain each, one bus: half a
        // microsecond of work in all. Moving a gain to another thread
        // would cost more than the gain.
        let mut text = String::from("bus mix\noutput out channel=1\nconnect mix -> out\n");
        for input in 1..=8 {
            text.push_str(&format!(
                "input in{input} channel={input}\ngain g{input} db=-6\n\
                 connect in{input} -> g{input} -> mix\n"
            ));
        }
        let graph = compiled(&text, 4);
        check(&graph);
        assert!(graph.nodes.iter().all(|node| node.thread == 0));
        assert_eq!(graph.makespan_ns, graph.work_ns);
    }

    #[test]
    fn a_node_that_waits_for_two_threads_starts_after_both() {
        let graph = compiled(
            "noise a\nnoise b seed=2\nload la class=heavy\nload lb class=heavy\nbus mix\npeak p\n\
             connect a -> la:0 -> mix -> p\nconnect b -> lb:0 -> mix\n",
            2,
        );
        check(&graph);
        let (la, lb, mix) = (
            place(&graph, "la"),
            place(&graph, "lb"),
            place(&graph, "mix"),
        );
        assert_ne!(graph.nodes[la].thread, graph.nodes[lb].thread);
        assert_eq!(graph.nodes[mix].waits_for.len(), 2);
        assert_eq!(graph.nodes[mix].inputs.len(), 2);
        // One of its inputs comes from the other thread: the loss is
        // counted.
        let ready = |node: usize| graph.nodes[node].start_ns + graph.nodes[node].cost_ns;
        assert!(graph.nodes[mix].start_ns >= ready(la).max(ready(lb)) + 1000);
    }

    #[test]
    fn the_card_is_on_thread_zero_and_a_pin_is_kept() {
        let graph = compiled(
            "input in1 channel=1\nload heavy class=heavy thread=3\noutput out1 channel=1\n\
             connect in1 -> heavy:0 -> out1\n",
            4,
        );
        check(&graph);
        assert_eq!(graph.nodes[place(&graph, "heavy")].thread, 3);
        assert_eq!(graph.nodes[place(&graph, "in1")].thread, 0);
        assert_eq!(graph.nodes[place(&graph, "out1")].thread, 0);
        // Input 1 of the load is left alone: said, not refused. Its
        // output 1 goes nowhere, but output 0 is taken: nothing to say.
        assert_eq!(graph.warnings.len(), 1, "{:?}", graph.warnings);
        assert!(
            graph.warnings[0].contains("input 1"),
            "{:?}",
            graph.warnings
        );
        let alone = compiled("noise n\nsum s\n", 2);
        assert_eq!(alone.warnings.len(), 3, "{:?}", alone.warnings);
        assert!(alone
            .warnings
            .iter()
            .any(|warning| warning.contains("it gives silence")));
        assert!(alone
            .warnings
            .iter()
            .any(|warning| warning.contains("computed for nothing")));
    }

    #[test]
    fn what_is_wrong_is_said_with_the_line() {
        let cases = [
            ("", 0, "has no node"),
            ("gain a thread=4\n", 1, "there are 4 live threads"),
            ("input a channel=1 thread=1\n", 1, "runs on thread 0"),
            (
                "output a channel=2\noutput b channel=2\n",
                2,
                "already the output 'a' (line 1)",
            ),
            ("peak p\ngain g\nconnect p -> g\n", 3, "it has no output"),
            ("noise n\ngain g\nconnect g -> n\n", 3, "it has no input"),
            (
                "noise n\ngain g\nconnect n -> g:1\n",
                3,
                "there is no input 1",
            ),
            (
                "noise n\ngain g\nconnect n:1 -> g\n",
                3,
                "there is no output 1",
            ),
            (
                "noise n\nnoise m seed=2\ngain g\nconnect n -> g\nconnect m -> g\n",
                5,
                "already connected on line 4",
            ),
            (
                "noise n\nsum s\nconnect n -> s:0\n",
                3,
                "its inputs have no number",
            ),
            (
                "gain a\ngain b\nconnect a -> b -> a\n",
                1,
                "wait for each other in a circle",
            ),
            ("gain a\nconnect a -> a\n", 1, "in a circle"),
        ];
        for (text, line, expected) in cases {
            let error = compile(&parse(text).unwrap(), &settings(4)).unwrap_err();
            assert_eq!(error.line, line, "{text}");
            assert!(error.message.contains(expected), "{}", error.message);
        }
    }

    #[test]
    fn a_circle_names_only_the_nodes_caught_in_it() {
        let error = compile(
            &parse("noise n\ngain a\ngain b\npeak p\nconnect n -> p\nconnect a -> b -> a\n")
                .unwrap(),
            &settings(2),
        )
        .unwrap_err();
        assert!(error.message.contains("a, b"), "{}", error.message);
        assert!(!error.message.contains("n,"), "{}", error.message);
    }

    #[test]
    fn the_same_description_compiles_the_same_way() {
        let text = "noise a\nnoise b seed=2\nload la class=medium\nload lb class=light\nsum s\n\
                    peak p\nconnect a -> la:0 -> s -> p\nconnect b -> lb:1 -> s\n";
        assert_eq!(compiled(text, 3), compiled(text, 3));
    }

    /// Many graphs made by a rule, of every shape the rule gives: each
    /// must compile into something that holds together, on one thread
    /// and on many.
    #[test]
    fn graphs_of_many_shapes_hold_together() {
        // A small generator of numbers that are the same at every run.
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = |below: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % below
        };
        for _ in 0..200 {
            let nodes = 2 + next(40) as usize;
            let mut text = String::from("noise source\n");
            for index in 0..nodes {
                let class = ["light", "medium", "heavy"][next(3) as usize];
                text.push_str(&format!("load l{index} class={class}\n"));
                // Each input from the source or from an earlier load:
                // never a circle.
                for port in 0..2 {
                    let from = next(index as u64 + 1) as usize;
                    if from == 0 {
                        text.push_str(&format!("connect source -> l{index}:{port}\n"));
                    } else {
                        let out = next(2);
                        text.push_str(&format!("connect l{}:{out} -> l{index}:{port}\n", from - 1));
                    }
                }
            }
            text.push_str("bus end\npeak p\nconnect end -> p\n");
            for index in 0..nodes {
                text.push_str(&format!("connect l{index} -> end\n"));
            }
            let description = parse(&text).unwrap();
            let mut last = u64::MAX;
            for threads in [1, 2, 4] {
                let graph = compile(&description, &settings(threads)).unwrap();
                check(&graph);
                assert_eq!(graph.nodes.len(), nodes + 3);
                if threads == 1 {
                    assert_eq!(graph.makespan_ns, graph.work_ns);
                }
                // More threads never make the schedule on paper much
                // longer: at the most by what crossing threads loses.
                assert!(
                    graph.makespan_ns <= last.saturating_add(1000 * graph.nodes.len() as u64),
                    "{threads} threads: {} after {last}",
                    graph.makespan_ns
                );
                last = graph.makespan_ns;
            }
        }
    }

    #[test]
    fn the_summary_says_whether_the_graph_fits() {
        let light = compiled("noise n\npeak p\nconnect n -> p\n", 4);
        let summary = light.summary(150);
        assert!(
            summary.contains("graph_nodes: 2 (1 noise, 1 peak)"),
            "{summary}"
        );
        assert!(summary.contains("graph_budget_us: 1850.00"), "{summary}");
        assert!(summary.contains("the graph fits"), "{summary}");
        assert!(summary.contains("n -> p"), "{summary}");

        // Two hundred heavy loads in a row: 2.9 ms on any number of
        // threads.
        let mut text = String::from("noise n\n");
        let mut before = "n".to_string();
        for index in 0..200 {
            text.push_str(&format!(
                "load l{index} class=heavy\nconnect {before} -> l{index}:0\n"
            ));
            before = format!("l{index}");
        }
        text.push_str(&format!("peak p\nconnect {before} -> p\n"));
        let heavy = compiled(&text, 4);
        check(&heavy);
        assert!(heavy.summary(150).contains("THE GRAPH DOES NOT FIT"));
    }
}
