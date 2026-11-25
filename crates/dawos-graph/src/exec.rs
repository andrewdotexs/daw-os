//! A compiled graph as the live threads run it (step P1.7c).
//!
//! Every live thread goes through its own nodes, in the order the
//! compiler gave them, once per cycle. A node whose inputs come from
//! another thread is run when that thread has written them: each node
//! carries the number of the last cycle it was run for, and a thread
//! looks at the numbers of the nodes it waits for. Nothing here blocks,
//! allocates or calls the system: a thread that has to wait spins, and
//! the cycle of the engine bounds how long.
//!
//! The buffers and what a node keeps from one cycle to the next are
//! behind locks that are only ever *tried*. As the graph is built nobody
//! holds one when another asks: a buffer is written by one node, before
//! the nodes that read it are let through, and a node belongs to one
//! thread. A lock that cannot be had means one of two things: a thread
//! went wrong while holding it (the watchdog of ADR 0023 has taken it
//! off its CPU), or the cycles of two threads overlapped, one being a
//! whole period behind the other. Either way the node that asks goes on
//! without (silence for a reader, the samples of the cycle before left
//! in place for a writer), and the refusal is counted.
//!
//! A thread that is behind (its cycle began when the others were at the
//! next one already) finds the marks of a later cycle: what it waits
//! for is gone, and it ends its cycle there, to be back with the others
//! at the next. If it was let through a moment before the other thread
//! began to write again, it may read samples of the next cycle, or the
//! two may ask for the same buffer at once, and one is refused: a
//! glitch in a cycle that was late already, and counted as such by the
//! engine.
//!
//! Samples are 32-bit floating point, full scale at 1; a bus adds its
//! inputs in 64 bits. The card gives and takes 32-bit integers: 24-bit
//! samples go through an input and an output node unchanged, to the
//! bit.
//!
//! A load is real filtering, as much of it as costs what the load
//! declares: a cascade of all-pass sections on the vector unit (see
//! `dawos_rt::dsp`), with the number of stages and of trims the
//! calibration gives (ADR 0011, step P1.7d). What comes out of a load
//! is its input with the phases turned, no louder.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};

use dawos_rt::clock;
use dawos_rt::dsp::{Cascade, Frame, LANES};

use crate::compile::Compiled;
use crate::kind::{Costs, Kind, Pitch};

/// Full scale of a 32-bit sample of the card.
const FULL_SCALE: f32 = 2_147_483_648.0;

/// What a node keeps from one cycle to the next.
enum State {
    /// Nothing: a sum.
    Nothing,
    /// The channel of the card it reads, from 0.
    Input {
        channel: usize,
    },
    /// The channel of the card it writes, from 0, and the samples of
    /// this cycle until the thread that holds the card takes them.
    Output {
        channel: usize,
        staged: Vec<i32>,
    },
    Gain {
        factor: f32,
    },
    /// Where the sum is made, in 64 bits.
    Bus {
        sum: Vec<f64>,
    },
    /// The samples still to come out, and the place of the next.
    Delay {
        line: Vec<f32>,
        at: usize,
    },
    Sine {
        phase: f64,
        step: f64,
        level: f64,
    },
    Impulse {
        every: u32,
        until_next: u32,
        level: f32,
    },
    Noise {
        state: u64,
        level: f32,
    },
    /// The highest sample seen, without its sign.
    Peak {
        peak: f32,
    },
    /// The filtering of a load, and a frame of the vector unit for
    /// every frame of the quantum.
    Cascade {
        cascade: Cascade,
        work: Vec<Frame>,
    },
    /// Nanoseconds to keep the CPU busy for in a cycle.
    Spin {
        ns: u64,
    },
    /// Cycles left before it stops returning.
    Trap {
        left: u64,
    },
}

/// A node as it is run: where it sits in this graph, and its cell.
struct Live {
    name: String,
    kind: Kind,
    /// The live thread that runs it.
    thread: usize,
    /// The buffer of each input; `None`: silence.
    inputs: Vec<Option<usize>>,
    /// The buffer of each output.
    outputs: Vec<usize>,
    /// The nodes it waits for.
    waits_for: Vec<usize>,
    /// What it is from one cycle to the next. A graph that takes the
    /// place of another shares the cells of the nodes that stay: the
    /// same name, the same kind, the same parameters (step P1.7e).
    cell: Arc<Cell>,
}

/// What a node is from one cycle to the next, whatever graph it is in.
struct Cell {
    /// The cycle it was last run for, plus one; 0: never.
    done: AtomicU64,
    /// Given up by the watchdog: nobody runs it again, and nobody waits
    /// for it.
    abandoned: AtomicBool,
    state: Mutex<State>,
    /// How often it was run, for how long in all and at the most, in
    /// nanoseconds: written by the thread that runs it.
    runs: AtomicU64,
    sum_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl std::ops::Deref for Live {
    type Target = Cell;

    fn deref(&self) -> &Cell {
        &self.cell
    }
}

/// How long a node took, over the cycles it was run in.
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    /// The name of the node in the description.
    pub name: String,
    /// Its kind, with its parameters.
    pub kind: Kind,
    /// The live thread that runs it in this graph.
    pub thread: usize,
    /// Cycles it was run in.
    pub runs: u64,
    /// Nanoseconds in all.
    pub sum_ns: u64,
    /// Nanoseconds of the longest run.
    pub max_ns: u64,
}

/// How the inputs of a node stand for a cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inputs {
    /// Written, every one of them: the node can be run.
    Ready,
    /// A node it waits for has not been run for this cycle yet.
    NotYet,
    /// A node it waits for is already at a later cycle: the thread that
    /// asks is behind, and what it waits for will not come.
    Gone,
}

/// A graph in the form the live threads run.
pub struct Running {
    nodes: Vec<Live>,
    threads: Vec<Vec<usize>>,
    /// The nodes that read the card and those that write it.
    inputs: Vec<usize>,
    outputs: Vec<usize>,
    buffers: Vec<RwLock<Vec<f32>>>,
    /// The buffers a node given up was working on: the thread stuck in
    /// it holds them for good. They read as silence, and what is
    /// written to them is dropped, without asking.
    lost: Vec<AtomicBool>,
    /// A quantum of silence, for an input nothing is connected to.
    silence: Vec<f32>,
    /// What the graph was compiled for: a graph that takes its place
    /// keeps the state of its nodes only if these are the same.
    rate: u32,
    frames: u32,
    costs: Costs,
    /// Nodes that took their cell from the graph before.
    kept: usize,
    /// Buffers and states that could not be had.
    refused: AtomicU64,
    /// Outputs of the card left silent because their node had not been
    /// run when the card was served.
    late_outputs: AtomicU64,
}

/// From decibels to a factor.
fn factor(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// What a node of kind `kind` starts from.
fn fresh(kind: &Kind, compiled: &Compiled) -> State {
    let frames = compiled.frames as usize;
    match *kind {
        Kind::Input { channel } => State::Input {
            channel: channel as usize - 1,
        },
        Kind::Output { channel } => State::Output {
            channel: channel as usize - 1,
            staged: vec![0; frames],
        },
        // Samples are 32-bit: the factor is too.
        Kind::Gain { db } => State::Gain {
            factor: factor(db) as f32,
        },
        Kind::Sum => State::Nothing,
        Kind::Bus => State::Bus {
            sum: vec![0.0; frames],
        },
        Kind::Delay { frames: by } => State::Delay {
            line: vec![0.0; by as usize],
            at: 0,
        },
        Kind::Sine { pitch, db } => State::Sine {
            phase: 0.0,
            step: match pitch {
                Pitch::Hz(hz) => TAU * hz / f64::from(compiled.rate),
                Pitch::Cycles(cycles) => TAU * f64::from(cycles) / f64::from(compiled.frames),
            },
            level: factor(db),
        },
        Kind::Impulse { every, db } => State::Impulse {
            every,
            until_next: 0,
            level: factor(db) as f32,
        },
        Kind::Noise { db, seed } => State::Noise {
            // Never zero, and far apart for seeds that are
            // close.
            state: (seed.wrapping_add(1)).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
            level: factor(db) as f32,
        },
        Kind::Peak => State::Peak { peak: 0.0 },
        Kind::Load { ns_per_frame } => {
            let (stages, trims) = compiled.costs.load_shape(ns_per_frame);
            State::Cascade {
                cascade: Cascade::new(stages, trims),
                work: vec![[0.0; LANES]; frames],
            }
        }
        Kind::Cascade { stages, trims } => State::Cascade {
            cascade: Cascade::new(stages, trims),
            work: vec![[0.0; LANES]; frames],
        },
        Kind::Spin { ns_per_frame } => State::Spin {
            ns: u64::from(ns_per_frame) * u64::from(compiled.frames),
        },
        Kind::Trap { after_s } => State::Trap {
            left: u64::from(after_s) * u64::from(compiled.rate) / u64::from(compiled.frames.max(1)),
        },
    }
}

impl Running {
    /// The graph `compiled`, ready for its first cycle. Everything a
    /// cycle needs is allocated here.
    #[must_use]
    pub fn new(compiled: &Compiled) -> Self {
        Self::build(compiled, None)
    }

    /// The graph `compiled`, to take the place of `previous` at the
    /// boundary of a period. A node with the name, the kind and the
    /// parameters of a node of `previous` goes on with what that node
    /// was: the memory of its filters, its delay line, its phase, its
    /// peak. Every other node starts afresh. Nothing is kept when the
    /// two graphs were compiled for another rate, another quantum or
    /// other costs. Outside the real-time path: it allocates.
    #[must_use]
    pub fn after(compiled: &Compiled, previous: &Self) -> Self {
        Self::build(compiled, Some(previous))
    }

    fn build(compiled: &Compiled, previous: Option<&Self>) -> Self {
        let frames = compiled.frames as usize;
        let previous = previous.filter(|previous| {
            previous.rate == compiled.rate
                && previous.frames == compiled.frames
                && previous.costs == compiled.costs
        });
        let mut kept = 0;
        let nodes: Vec<Live> = compiled
            .nodes
            .iter()
            .map(|node| {
                let stays = previous.and_then(|previous| {
                    previous
                        .nodes
                        .iter()
                        .find(|old| old.name == node.name && old.kind == node.kind)
                });
                let cell = match stays {
                    Some(old) => {
                        kept += 1;
                        Arc::clone(&old.cell)
                    }
                    None => Arc::new(Cell {
                        done: AtomicU64::new(0),
                        abandoned: AtomicBool::new(false),
                        state: Mutex::new(fresh(&node.kind, compiled)),
                        runs: AtomicU64::new(0),
                        sum_ns: AtomicU64::new(0),
                        max_ns: AtomicU64::new(0),
                    }),
                };
                Live {
                    name: node.name.clone(),
                    kind: node.kind,
                    thread: node.thread,
                    inputs: node.inputs.clone(),
                    outputs: node.outputs.clone(),
                    waits_for: node.waits_for.clone(),
                    cell,
                }
            })
            .collect();
        let of_kind = |wanted: fn(&Kind) -> bool| -> Vec<usize> {
            compiled
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| wanted(&node.kind))
                .map(|(place, _)| place)
                .collect()
        };
        Self {
            nodes,
            threads: compiled.threads.clone(),
            inputs: of_kind(|kind| matches!(kind, Kind::Input { .. })),
            outputs: of_kind(|kind| matches!(kind, Kind::Output { .. })),
            buffers: (0..compiled.buffers)
                .map(|_| RwLock::new(vec![0.0; frames]))
                .collect(),
            lost: (0..compiled.buffers)
                .map(|_| AtomicBool::new(false))
                .collect(),
            silence: vec![0.0; frames],
            rate: compiled.rate,
            frames: compiled.frames,
            costs: compiled.costs,
            kept,
            refused: AtomicU64::new(0),
            late_outputs: AtomicU64::new(0),
        }
    }

    /// Nodes that went on from the graph before with what they were.
    #[must_use]
    pub fn kept(&self) -> usize {
        self.kept
    }

    /// Nodes in the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Node `node` took `ns` nanoseconds in a cycle. By the thread that
    /// ran it: three stores, no lock.
    #[inline]
    pub fn record(&self, node: usize, ns: u64) {
        let cell = &self.nodes[node].cell;
        cell.runs
            .store(cell.runs.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        cell.sum_ns
            .store(cell.sum_ns.load(Ordering::Relaxed) + ns, Ordering::Relaxed);
        if ns > cell.max_ns.load(Ordering::Relaxed) {
            cell.max_ns.store(ns, Ordering::Relaxed);
        }
    }

    /// How long every node took so far, in the order of the compiled
    /// graph. A node that came from the graph before brings its times
    /// with it. Not for a live thread: it allocates.
    #[must_use]
    pub fn timed(&self) -> Vec<Timed> {
        self.nodes
            .iter()
            .map(|node| Timed {
                name: node.name.clone(),
                kind: node.kind,
                thread: node.thread,
                runs: node.runs.load(Ordering::Relaxed),
                sum_ns: node.sum_ns.load(Ordering::Relaxed),
                max_ns: node.max_ns.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// The nodes of live thread `thread`, in the order it runs them;
    /// none for a thread the graph was not compiled for.
    #[inline]
    #[must_use]
    pub fn nodes_of(&self, thread: usize) -> &[usize] {
        self.threads.get(thread).map_or(&[], Vec::as_slice)
    }

    /// Whether node `node` is an input of the card: [`Running::capture`]
    /// runs those, not the loop of the thread.
    #[inline]
    #[must_use]
    pub fn is_input(&self, node: usize) -> bool {
        self.inputs.contains(&node)
    }

    /// Whether the watchdog gave node `node` up.
    #[inline]
    #[must_use]
    pub fn is_abandoned(&self, node: usize) -> bool {
        self.nodes[node].abandoned.load(Ordering::Acquire)
    }

    /// Give node `node` up: nobody runs it again, and what waits for it
    /// goes on with silence. The thread stuck in it holds the buffers
    /// it reads and writes, and nobody can take them from it: from now
    /// on they are silence for every node that reads them, until a
    /// graph without the node is in use (step P1.7f). By the watchdog:
    /// a few stores, no lock.
    pub fn abandon(&self, node: usize) {
        let live = &self.nodes[node];
        for &buffer in live.inputs.iter().flatten().chain(&live.outputs) {
            self.lost[buffer].store(true, Ordering::Release);
        }
        live.abandoned.store(true, Ordering::Release);
    }

    /// The name of node `node` in the description.
    #[must_use]
    pub fn name_of(&self, node: usize) -> &str {
        &self.nodes[node].name
    }

    /// The names of the nodes given up. Not for a live thread: it
    /// allocates.
    #[must_use]
    pub fn given_up(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|node| node.abandoned.load(Ordering::Acquire))
            .map(|node| node.name.clone())
            .collect()
    }

    #[inline]
    fn is_lost(&self, buffer: usize) -> bool {
        self.lost[buffer].load(Ordering::Relaxed)
    }

    /// Whether everything node `node` waits for was run for cycle
    /// `cycle`, or given up.
    #[inline]
    #[must_use]
    pub fn ready(&self, node: usize, cycle: u64) -> bool {
        self.inputs_of(node, cycle) == Inputs::Ready
    }

    /// How the inputs of node `node` stand for cycle `cycle`.
    #[inline]
    #[must_use]
    pub fn inputs_of(&self, node: usize, cycle: u64) -> Inputs {
        let mut inputs = Inputs::Ready;
        for &other in &self.nodes[node].waits_for {
            let other = &self.nodes[other];
            if other.abandoned.load(Ordering::Acquire) {
                continue;
            }
            let done = other.done.load(Ordering::Acquire);
            if done > cycle + 1 {
                return Inputs::Gone;
            }
            if done != cycle + 1 {
                inputs = Inputs::NotYet;
            }
        }
        inputs
    }

    #[inline]
    fn refuse(&self) {
        self.refused.fetch_add(1, Ordering::Relaxed);
    }

    /// Run `work` on the buffer of output `port` of `node`.
    #[inline]
    fn write(&self, node: &Live, port: usize, work: impl FnOnce(&mut [f32])) {
        let buffer = node.outputs[port];
        if self.is_lost(buffer) {
            return;
        }
        match self.buffers[buffer].try_write() {
            Ok(mut buffer) => work(&mut buffer),
            Err(_) => self.refuse(),
        }
    }

    /// Run `work` on the buffer input `port` of `node` reads; not at all
    /// when nothing is connected there, or when the buffer cannot be
    /// had. Returns whether it ran.
    #[inline]
    fn read(&self, node: &Live, port: usize, work: impl FnOnce(&[f32])) -> bool {
        let Some(buffer) = node.inputs.get(port).copied().flatten() else {
            return false;
        };
        if self.is_lost(buffer) {
            return false;
        }
        match self.buffers[buffer].try_read() {
            Ok(buffer) => {
                work(&buffer);
                true
            }
            Err(_) => {
                self.refuse();
                false
            }
        }
    }

    /// The buffer input `port` of `node` reads, held for reading; none
    /// when nothing is connected there, or when it cannot be had.
    #[inline]
    fn input(&self, node: &Live, port: usize) -> Option<RwLockReadGuard<'_, Vec<f32>>> {
        let buffer = node.inputs.get(port).copied().flatten()?;
        if self.is_lost(buffer) {
            return None;
        }
        match self.buffers[buffer].try_read() {
            Ok(buffer) => Some(buffer),
            Err(_) => {
                self.refuse();
                None
            }
        }
    }

    /// From input 0 to output 0, sample by sample; silence in when
    /// nothing is connected.
    #[inline]
    fn each(&self, node: &Live, mut sample: impl FnMut(f32) -> f32) {
        self.write(node, 0, |output| {
            let ran = self.read(node, 0, |input| {
                for (out, &given) in output.iter_mut().zip(input) {
                    *out = sample(given);
                }
            });
            if !ran {
                for out in output.iter_mut() {
                    *out = sample(0.0);
                }
            }
        });
    }

    /// The inputs of the card for cycle `cycle`: `input` is what the
    /// card gave, one quantum, `channels` channels one after the other
    /// in every frame. A channel the card does not have, and every
    /// channel when there is no card (`channels` 0), is silence. Called
    /// by the thread that holds the card, before its nodes.
    pub fn capture(&self, cycle: u64, input: &[i32], channels: usize) {
        for &place in &self.inputs {
            let node = &self.nodes[place];
            let Ok(state) = node.state.try_lock() else {
                self.refuse();
                continue;
            };
            if let State::Input { channel } = *state {
                self.write(node, 0, |buffer| {
                    if channel < channels {
                        for (frame, out) in buffer.iter_mut().enumerate() {
                            // A 24-bit sample is a whole number of 256ths
                            // of a 32-bit one, and fits a float exactly.
                            *out = input.get(frame * channels + channel).copied().unwrap_or(0)
                                as f32
                                / FULL_SCALE;
                        }
                    } else {
                        buffer.fill(0.0);
                    }
                });
            }
            drop(state);
            node.done.store(cycle + 1, Ordering::Release);
        }
    }

    /// Run node `place` for cycle `cycle`. Compute phase: no system
    /// call, no allocation, no wait.
    pub fn run(&self, place: usize, cycle: u64) {
        let node = &self.nodes[place];
        let Ok(mut state) = node.state.try_lock() else {
            // Whoever holds it is stuck in this node.
            self.refuse();
            return;
        };
        match &mut *state {
            // Run by `capture`.
            State::Input { .. } => {}
            State::Output { staged, .. } => {
                let ran = self.read(node, 0, |input| {
                    for (out, &given) in staged.iter_mut().zip(input) {
                        // The clamp keeps the cast inside a 32-bit sample.
                        *out = (given * FULL_SCALE).clamp(-FULL_SCALE, FULL_SCALE - 128.0) as i32;
                    }
                });
                if !ran {
                    staged.fill(0);
                }
            }
            State::Gain { factor } => {
                let factor = *factor;
                self.each(node, |sample| sample * factor);
            }
            State::Nothing => self.write(node, 0, |output| {
                output.fill(0.0);
                for port in 0..node.inputs.len() {
                    self.read(node, port, |input| {
                        for (out, &given) in output.iter_mut().zip(input) {
                            *out += given;
                        }
                    });
                }
            }),
            State::Bus { sum } => {
                sum.fill(0.0);
                for port in 0..node.inputs.len() {
                    self.read(node, port, |input| {
                        for (total, &given) in sum.iter_mut().zip(input) {
                            *total += f64::from(given);
                        }
                    });
                }
                self.write(node, 0, |output| {
                    for (out, &total) in output.iter_mut().zip(sum.iter()) {
                        // Back to the 32 bits of every buffer.
                        *out = total as f32;
                    }
                });
            }
            State::Delay { line, at } => self.each(node, |sample| {
                let late = line[*at];
                line[*at] = sample;
                // Not a remainder: a division for every sample.
                *at += 1;
                if *at == line.len() {
                    *at = 0;
                }
                late
            }),
            State::Sine { phase, step, level } => self.write(node, 0, |output| {
                for out in output.iter_mut() {
                    *out = (*level * phase.sin()) as f32;
                    *phase += *step;
                    if *phase >= TAU {
                        *phase -= TAU;
                    }
                }
            }),
            State::Impulse {
                every,
                until_next,
                level,
            } => self.write(node, 0, |output| {
                for out in output.iter_mut() {
                    if *until_next == 0 {
                        *out = *level;
                        *until_next = *every - 1;
                    } else {
                        *out = 0.0;
                        *until_next -= 1;
                    }
                }
            }),
            State::Noise { state, level } => self.write(node, 0, |output| {
                for out in output.iter_mut() {
                    // xorshift64*: enough for a test signal.
                    *state ^= *state >> 12;
                    *state ^= *state << 25;
                    *state ^= *state >> 27;
                    let bits = state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40;
                    // 24 bits: from -1 to just under 1.
                    *out = (bits as f32 / 8_388_608.0 - 1.0) * *level;
                }
            }),
            State::Peak { peak } => {
                self.read(node, 0, |input| {
                    for &given in input {
                        *peak = peak.max(given.abs());
                    }
                });
            }
            State::Cascade { cascade, work } => {
                let left = self.input(node, 0);
                let right = self.input(node, 1);
                let left = left.as_deref().map_or(&self.silence[..], Vec::as_slice);
                let right = right.as_deref().map_or(&self.silence[..], Vec::as_slice);
                if self.is_lost(node.outputs[0]) || self.is_lost(node.outputs[1]) {
                    // Nowhere to put it.
                } else {
                    match (
                        self.buffers[node.outputs[0]].try_write(),
                        self.buffers[node.outputs[1]].try_write(),
                    ) {
                        (Ok(mut out_left), Ok(mut out_right)) => {
                            cascade.run(left, right, &mut out_left, &mut out_right, work);
                        }
                        _ => self.refuse(),
                    }
                }
            }
            State::Spin { ns } => {
                let end = clock::now() + *ns;
                self.each(node, |sample| sample);
                while clock::now() < end {
                    std::hint::spin_loop();
                }
            }
            State::Trap { left } => {
                if *left == 0 {
                    // Gone wrong, as a node stuck in its work is: its
                    // state, its output and its input held, and no
                    // return. Only the watchdog ends this for the rest
                    // of the engine.
                    let _output = self.buffers[node.outputs[0]].try_write();
                    let _input = self.input(node, 0);
                    loop {
                        std::hint::spin_loop();
                    }
                }
                *left -= 1;
                self.each(node, |sample| sample);
            }
        }
        drop(state);
        node.done.store(cycle + 1, Ordering::Release);
    }

    /// What the graph plays in cycle `cycle`, into `output`: one
    /// quantum, `channels` channels one after the other in every frame.
    /// The channel of an output node that was not run for this cycle is
    /// left as it is, and counted. Called by the thread that holds the
    /// card, at the end of its compute phase. Returns how many were
    /// left.
    pub fn deliver(&self, cycle: u64, output: &mut [i32], channels: usize) -> u32 {
        let mut late = 0;
        for &place in &self.outputs {
            let node = &self.nodes[place];
            if node.done.load(Ordering::Acquire) != cycle + 1 {
                late += 1;
                continue;
            }
            let Ok(state) = node.state.try_lock() else {
                self.refuse();
                late += 1;
                continue;
            };
            if let State::Output { channel, staged } = &*state {
                if *channel < channels {
                    for (frame, &sample) in staged.iter().enumerate() {
                        if let Some(out) = output.get_mut(frame * channels + channel) {
                            *out = sample;
                        }
                    }
                }
            }
        }
        if late > 0 {
            self.late_outputs
                .fetch_add(u64::from(late), Ordering::Relaxed);
        }
        late
    }

    /// Buffers and states that were in use by another thread when one
    /// asked: none in a healthy run.
    #[must_use]
    pub fn refused(&self) -> u64 {
        self.refused.load(Ordering::Relaxed)
    }

    /// Outputs of the card left silent for a cycle because their node
    /// had not been run in time.
    #[must_use]
    pub fn late_outputs(&self) -> u64 {
        self.late_outputs.load(Ordering::Relaxed)
    }

    /// The peak every meter saw, with the name of its node. Not for a
    /// live thread: it allocates.
    #[must_use]
    pub fn peaks(&self) -> Vec<(String, f32)> {
        self.nodes
            .iter()
            .filter_map(|node| match node.state.try_lock() {
                Ok(state) => match *state {
                    State::Peak { peak } => Some((node.name.clone(), peak)),
                    _ => None,
                },
                Err(_) => None,
            })
            .collect()
    }

    /// One cycle on the calling thread alone, every node in an order in
    /// which each comes after what it waits for: for tests, and for a
    /// graph run outside the engine.
    pub fn run_all(&self, cycle: u64, input: &[i32], output: &mut [i32], channels: (usize, usize)) {
        self.capture(cycle, input, channels.0);
        for place in 0..self.nodes.len() {
            if !self.is_input(place) && !self.is_abandoned(place) {
                self.run(place, cycle);
            }
        }
        self.deliver(cycle, output, channels.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{compile, Settings};
    use crate::kind::Costs;
    use crate::text::parse;

    const FRAMES: u32 = 96;

    fn running(text: &str, threads: usize) -> Running {
        let settings = Settings {
            threads,
            rate: 48_000,
            frames: FRAMES,
            costs: Costs::default(),
        };
        Running::new(&compile(&parse(text).unwrap(), &settings).unwrap())
    }

    fn peak_db(graph: &Running, name: &str) -> f64 {
        let peak = graph
            .peaks()
            .into_iter()
            .find(|(node, _)| node == name)
            .unwrap()
            .1;
        20.0 * f64::from(peak).log10()
    }

    #[test]
    fn a_sine_through_a_gain_reaches_the_meter_six_decibels_down() {
        let graph = running(
            "sine tone cycles=3 db=-12\ngain soft db=-6\npeak before\npeak after\n\
             connect tone -> soft -> after\nconnect tone -> before\n",
            1,
        );
        for cycle in 0..10 {
            graph.run_all(cycle, &[], &mut [], (0, 0));
        }
        assert!((peak_db(&graph, "before") + 12.0).abs() < 0.01);
        assert!((peak_db(&graph, "after") + 18.0).abs() < 0.01);
        assert_eq!(graph.refused(), 0);
        assert_eq!(graph.late_outputs(), 0);
    }

    #[test]
    fn samples_of_24_bits_cross_the_graph_to_the_bit() {
        // Input 2 of a card with three channels to output 1 of a card
        // with two.
        let graph = running(
            "input in2 channel=2\noutput out1 channel=1\nconnect in2 -> out1\n",
            1,
        );
        let frames = FRAMES as usize;
        let mut input = vec![0_i32; frames * 3];
        for frame in 0..frames {
            // Every 24-bit pattern the frame number gives, both signs,
            // and the two ends.
            let sample: i32 = match frame {
                0 => i32::MIN,
                1 => i32::MAX & !0xff,
                _ => (frame as i32 * 0x0001_2345 - 0x0040_0000) << 8,
            };
            input[frame * 3 + 1] = sample;
            input[frame * 3] = 0x7fff_ff00;
            input[frame * 3 + 2] = -0x7fff_ff00;
        }
        let mut output = vec![0x1234_i32; frames * 2];
        graph.run_all(0, &input, &mut output, (3, 2));
        for frame in 0..frames {
            assert_eq!(output[frame * 2], input[frame * 3 + 1], "frame {frame}");
            // The other channel is nobody's: left as it was.
            assert_eq!(output[frame * 2 + 1], 0x1234);
        }
    }

    #[test]
    fn a_bus_adds_and_a_sum_adds() {
        let graph = running(
            "impulse a every=1 db=-6\nimpulse b every=1 db=-6\nbus wide\nsum narrow\n\
             peak on_bus\npeak on_sum\nconnect a -> wide -> on_bus\nconnect b -> wide\n\
             connect a -> narrow -> on_sum\nconnect b -> narrow\n",
            1,
        );
        graph.run_all(0, &[], &mut [], (0, 0));
        // Twice the same signal: 6.02 dB more.
        assert!((peak_db(&graph, "on_bus") + 6.0 - 6.0206).abs() < 0.001);
        assert!((peak_db(&graph, "on_sum") + 6.0 - 6.0206).abs() < 0.001);
    }

    #[test]
    fn a_delay_gives_back_what_it_took_that_many_frames_later() {
        // An impulse every 1000 frames, delayed by 100, to output 1.
        let graph = running(
            "impulse click every=1000 db=0\ndelay late frames=100\noutput out channel=1\n\
             connect click -> late -> out\n",
            1,
        );
        let frames = FRAMES as usize;
        let mut heard = Vec::new();
        for cycle in 0..25 {
            let mut output = vec![0_i32; frames];
            graph.run_all(cycle, &[], &mut output, (0, 1));
            for (frame, &sample) in output.iter().enumerate() {
                if sample != 0 {
                    heard.push(cycle as usize * frames + frame);
                }
            }
        }
        assert_eq!(heard, [100, 1100, 2100]);
    }

    #[test]
    fn noise_is_the_same_at_every_run_and_differs_by_seed() {
        let play = |seed: u32| {
            let graph = running(
                &format!("noise n seed={seed} db=0\noutput out channel=1\nconnect n -> out\n"),
                1,
            );
            let mut output = vec![0_i32; FRAMES as usize];
            graph.run_all(0, &[], &mut output, (0, 1));
            output
        };
        assert_eq!(play(7), play(7));
        assert_ne!(play(7), play(8));
        let loud = play(7)
            .iter()
            .filter(|sample| sample.unsigned_abs() > 1 << 30)
            .count();
        assert!(loud > 20, "{loud} samples over half scale");
    }

    #[test]
    fn nothing_connected_is_silence_and_an_output_not_run_is_counted() {
        let graph = running(
            "gain alone db=6\noutput out channel=1\nconnect alone -> out\n",
            1,
        );
        let mut output = vec![5_i32; FRAMES as usize];
        graph.run_all(0, &[], &mut output, (0, 1));
        assert!(output.iter().all(|sample| *sample == 0));
        // The card is served for a cycle the output node was not run
        // for: its channel is left alone.
        let mut output = vec![5_i32; FRAMES as usize];
        assert_eq!(graph.deliver(1, &mut output, 1), 1);
        assert!(output.iter().all(|sample| *sample == 5));
        assert_eq!(graph.late_outputs(), 1);
    }

    /// A thread stuck in a node holds the state of the node and the
    /// buffers it was reading and writing, for good. Once the node is
    /// given up nobody asks for them again: what read them hears
    /// silence, and a graph without the node gives the rest back.
    #[test]
    fn a_node_given_up_leaves_silence_where_it_was_working() {
        let text = "sine tone cycles=3 db=-6\n\
                    trap bad after=1000\n\
                    gain beside\n\
                    output one channel=1\n\
                    output two channel=2\n\
                    connect tone -> bad -> one\n\
                    connect tone -> beside -> two\n";
        let settings = Settings {
            threads: 1,
            rate: 48_000,
            frames: FRAMES,
            costs: Costs::default(),
        };
        let description = parse(text).unwrap();
        let compiled = compile(&description, &settings).unwrap();
        let graph = Running::new(&compiled);
        let place = |name: &str| {
            graph
                .nodes
                .iter()
                .position(|node| node.name == name)
                .unwrap()
        };
        let (tone, bad) = (place("tone"), place("bad"));
        let frames = FRAMES as usize;
        let mut output = vec![0_i32; frames * 2];
        let heard = |output: &[i32], channel: usize| {
            (0..frames).any(|frame| output[frame * 2 + channel] != 0)
        };

        // Healthy: the tone on both outputs.
        graph.run_all(0, &[], &mut output, (0, 2));
        assert!(heard(&output, 0) && heard(&output, 1));
        assert_eq!(graph.refused(), 0);

        // The thread stuck in the trap, as far as the others can tell.
        let _state = graph.nodes[bad].state.try_lock().unwrap();
        let _input = graph.buffers[graph.nodes[tone].outputs[0]]
            .try_read()
            .unwrap();
        let _output = graph.buffers[graph.nodes[bad].outputs[0]]
            .try_write()
            .unwrap();
        // Before the watchdog notices, the others find things taken.
        graph.run(tone, 1);
        assert!(graph.refused() > 0);

        graph.abandon(bad);
        assert_eq!(graph.given_up(), ["bad".to_string()]);
        let refused = graph.refused();
        for cycle in 2..6 {
            output.fill(1);
            graph.run_all(cycle, &[], &mut output, (0, 2));
            assert!(!heard(&output, 0), "after the node: silence");
            assert!(
                !heard(&output, 1),
                "beside it, on the buffer it held: silence"
            );
        }
        assert_eq!(graph.refused(), refused, "nobody asked for what is lost");

        // A graph without the node: what was beside it is back, what
        // was after it stays silent.
        let without = compile(&description.without(&graph.given_up()), &settings).unwrap();
        let next = Running::after(&without, &graph);
        assert_eq!((next.kept(), next.len()), (4, 4));
        assert!(next.given_up().is_empty());
        output.fill(1);
        next.run_all(6, &[], &mut output, (0, 2));
        assert!(!heard(&output, 0));
        assert!(heard(&output, 1));
        assert_eq!(next.refused(), 0);
    }

    /// A trap is a copy until its time comes.
    #[test]
    fn a_trap_works_until_its_time() {
        let graph = running(
            "impulse click every=96 db=0\ntrap bad after=1\npeak p\nconnect click -> bad -> p\n",
            1,
        );
        let mut output = [0_i32; 0];
        // One second is 500 cycles of 96 frames: none of these is the
        // last.
        for cycle in 0..499 {
            graph.run_all(cycle, &[], &mut output, (0, 0));
        }
        assert!(peak_db(&graph, "p").abs() < 0.01);
        assert_eq!(graph.refused(), 0);
    }

    #[test]
    fn a_node_given_up_is_not_waited_for() {
        let graph = running("noise n\ngain g\npeak p\nconnect n -> g -> p\n", 1);
        let (n, g, p) = (0, 1, 2);
        assert!(!graph.ready(g, 0));
        graph.run(n, 0);
        assert!(graph.ready(g, 0));
        assert!(!graph.ready(g, 1), "the cycle before does not count");
        assert_eq!(graph.inputs_of(g, 1), Inputs::NotYet);
        graph.run(n, 3);
        assert_eq!(graph.inputs_of(g, 1), Inputs::Gone, "a later cycle");
        graph.abandon(g);
        assert!(graph.is_abandoned(g));
        assert!(graph.ready(p, 5));
    }

    #[test]
    fn a_load_filters_both_channels_and_is_no_louder() {
        // Noise on the left, an impulse on the right, through a load of
        // each class, to two outputs.
        let graph = running(
            "noise n db=-6\nimpulse i every=7 db=-6\nload a class=light\nload b class=medium\n\
             load c class=heavy\noutput l channel=1\noutput r channel=2\npeak in\npeak out\n\
             connect n -> a:0 -> b:0 -> c:0 -> l\nconnect i -> a:1\nconnect a:1 -> b:1\n\
             connect b:1 -> c:1\nconnect c:1 -> r\nconnect n -> in\nconnect c:0 -> out\n",
            1,
        );
        let frames = FRAMES as usize;
        let mut power = (0.0_f64, 0.0_f64);
        for cycle in 0..50 {
            let mut output = vec![0_i32; frames * 2];
            graph.run_all(cycle, &[], &mut output, (0, 2));
            for frame in output.chunks(2) {
                power.0 += (f64::from(frame[0]) / f64::from(FULL_SCALE)).powi(2);
                power.1 += (f64::from(frame[1]) / f64::from(FULL_SCALE)).powi(2);
            }
        }
        // Both channels carry something, and not the same thing.
        assert!(power.0 > 1.0 && power.1 > 0.1, "{power:?}");
        assert!((power.0 - power.1).abs() > 0.1);
        // No louder than what went in, by more than the phases allow.
        assert!(peak_db(&graph, "out") < peak_db(&graph, "in") + 6.0);
        assert_eq!(graph.refused(), 0);
    }

    #[test]
    fn a_cascade_of_nothing_but_trims_and_a_spin_give_back_their_input() {
        let graph = running(
            "impulse i every=5 db=-3\ncascade c stages=0 trims=3\nspin s ns=5\n\
             output a channel=1\noutput b channel=2\n\
             connect i -> c:0 -> a\nconnect i -> s -> b\n",
            1,
        );
        let mut output = vec![0_i32; FRAMES as usize * 2];
        graph.run_all(0, &[], &mut output, (0, 2));
        for (frame, pair) in output.chunks(2).enumerate() {
            assert_eq!(pair[0], pair[1], "frame {frame}");
            assert_eq!(pair[0] != 0, frame % 5 == 0, "frame {frame}");
        }
    }

    fn compiled_for(text: &str, frames: u32) -> Compiled {
        let settings = Settings {
            threads: 1,
            rate: 48_000,
            frames,
            costs: Costs::default(),
        };
        compile(&parse(text).unwrap(), &settings).unwrap()
    }

    /// Where a click every 1000 frames, delayed by 100, is heard on
    /// output 1 over 25 cycles: `first` runs the first twelve, and
    /// `second`, when there is one, takes its place for the others.
    fn clicks(first: &Running, second: Option<&Running>) -> Vec<usize> {
        let frames = FRAMES as usize;
        let mut heard = Vec::new();
        for cycle in 0..25 {
            let graph = second.filter(|_| cycle >= 12).unwrap_or(first);
            let mut output = vec![0_i32; frames];
            graph.run_all(cycle, &[], &mut output, (0, 1));
            for (frame, &sample) in output.iter().enumerate() {
                if sample != 0 {
                    heard.push(cycle as usize * frames + frame);
                }
            }
        }
        heard
    }

    const CLICK: &str = "impulse click every=1000 db=0\ndelay late frames=100\n\
                         output out channel=1\nconnect click -> late -> out\n";

    #[test]
    fn a_graph_that_takes_the_place_of_another_goes_on_where_it_was() {
        // The same chain, and two nodes more.
        let more = format!("{CLICK}noise extra\npeak seen\nconnect extra -> seen\n");
        let first = Running::new(&compiled_for(CLICK, FRAMES));
        let second = Running::after(&compiled_for(&more, FRAMES), &first);
        assert_eq!((second.len(), second.kept()), (5, 3));
        // The impulse counts on and the delay line gives back what it
        // held: the clicks fall where they would have without a change.
        assert_eq!(clicks(&first, Some(&second)), [100, 1100, 2100]);
        // And the nodes that stayed bring their times with them.
        let place = |graph: &Running, name: &str| {
            graph
                .timed()
                .iter()
                .position(|node| node.name == name)
                .unwrap()
        };
        first.record(place(&first, "late"), 30);
        second.record(place(&second, "late"), 50);
        let timed = &second.timed()[place(&second, "late")];
        assert_eq!((timed.runs, timed.sum_ns, timed.max_ns), (2, 80, 50));
        assert_eq!(second.timed()[place(&second, "seen")].runs, 0);
    }

    #[test]
    fn a_node_that_changed_starts_afresh() {
        // The delay is another: 50 frames. The impulse and the output
        // stay, the line is empty again.
        let shorter = CLICK.replace("frames=100", "frames=50");
        let first = Running::new(&compiled_for(CLICK, FRAMES));
        let second = Running::after(&compiled_for(&shorter, FRAMES), &first);
        assert_eq!(second.kept(), 2);
        // The click of frame 2000 goes into the new line: it comes 50
        // frames later, not 100. (The one of frame 1000 was out of the
        // old line before the change.)
        assert_eq!(clicks(&first, Some(&second)), [100, 1100, 2050]);
        // Compiled for another quantum, nothing can be kept.
        let other = Running::after(&compiled_for(CLICK, 2 * FRAMES), &first);
        assert_eq!(other.kept(), 0);
    }

    /// Four threads, each with its own nodes, spinning for what they
    /// wait for as the engine does: every cycle every node is run, and
    /// the signal crosses the threads whole.
    #[test]
    fn four_threads_run_one_graph() {
        let mut text = String::from("bus mix\npeak meter\nconnect mix -> meter\n");
        for track in 0..8 {
            text.push_str(&format!(
                "impulse t{track} every=1 db=-30\nspin t{track}.a ns=20 thread={}\n\
                 spin t{track}.b ns=20 thread={}\n\
                 connect t{track} -> t{track}.a -> t{track}.b -> mix\n",
                track % 4,
                (track + 1) % 4
            ));
        }
        let graph = std::sync::Arc::new(running(&text, 4));
        const CYCLES: u64 = 200;
        let cycle = std::sync::Arc::new(AtomicU64::new(0));
        let finished = std::sync::Arc::new(AtomicU64::new(0));
        let threads: Vec<_> = (0..4)
            .map(|thread| {
                let graph = std::sync::Arc::clone(&graph);
                let cycle = std::sync::Arc::clone(&cycle);
                let finished = std::sync::Arc::clone(&finished);
                std::thread::spawn(move || {
                    for now in 0..CYCLES {
                        // All four begin a cycle together.
                        while cycle.load(Ordering::Acquire) != now {
                            std::thread::yield_now();
                        }
                        if thread == 0 {
                            graph.capture(now, &[], 0);
                        }
                        for &node in graph.nodes_of(thread) {
                            if graph.is_input(node) {
                                continue;
                            }
                            while !graph.ready(node, now) {
                                std::thread::yield_now();
                            }
                            graph.run(node, now);
                        }
                        // The last to finish opens the next cycle.
                        if finished.fetch_add(1, Ordering::AcqRel) % 4 == 3 {
                            cycle.store(now + 1, Ordering::Release);
                        }
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(graph.refused(), 0);
        // Eight impulses at -30 dB on the bus: 18 dB more.
        assert!(
            (peak_db(&graph, "meter") + 30.0 - 18.06).abs() < 0.05,
            "{}",
            peak_db(&graph, "meter")
        );
    }
}
