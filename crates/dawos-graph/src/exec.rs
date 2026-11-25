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
//! The load is a stand-in until step P1.7d: it copies its two inputs to
//! its two outputs and keeps the CPU busy for the time it is declared to
//! cost. The filtering it will do, calibrated against dawos-dspbench,
//! comes there.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use dawos_rt::clock;

use crate::compile::Compiled;
use crate::kind::{Kind, Pitch};

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
    /// Nanoseconds to keep the CPU busy for in a cycle.
    Load {
        ns: u64,
    },
}

/// A node as it is run.
struct Live {
    name: String,
    /// The buffer of each input; `None`: silence.
    inputs: Vec<Option<usize>>,
    /// The buffer of each output.
    outputs: Vec<usize>,
    /// The nodes it waits for.
    waits_for: Vec<usize>,
    /// The cycle it was last run for, plus one; 0: never.
    done: AtomicU64,
    /// Given up by the watchdog: nobody runs it again, and nobody waits
    /// for it.
    abandoned: AtomicBool,
    state: Mutex<State>,
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

impl Running {
    /// The graph `compiled`, ready for its first cycle. Everything a
    /// cycle needs is allocated here.
    #[must_use]
    pub fn new(compiled: &Compiled) -> Self {
        let frames = compiled.frames as usize;
        let nodes: Vec<Live> = compiled
            .nodes
            .iter()
            .map(|node| {
                let state = match node.kind {
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
                            Pitch::Cycles(cycles) => {
                                TAU * f64::from(cycles) / f64::from(compiled.frames)
                            }
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
                    Kind::Load { ns_per_frame } => State::Load {
                        ns: u64::from(ns_per_frame) * u64::from(compiled.frames),
                    },
                };
                Live {
                    name: node.name.clone(),
                    inputs: node.inputs.clone(),
                    outputs: node.outputs.clone(),
                    waits_for: node.waits_for.clone(),
                    done: AtomicU64::new(0),
                    abandoned: AtomicBool::new(false),
                    state: Mutex::new(state),
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
            refused: AtomicU64::new(0),
            late_outputs: AtomicU64::new(0),
        }
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
    /// goes on with silence.
    pub fn abandon(&self, node: usize) {
        self.nodes[node].abandoned.store(true, Ordering::Release);
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
        match self.buffers[node.outputs[port]].try_write() {
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
                *at = (*at + 1) % line.len();
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
            State::Load { ns } => {
                let end = clock::now() + *ns;
                for port in 0..2 {
                    self.write(node, port, |output| {
                        if !self.read(node, port, |input| output.copy_from_slice(input)) {
                            output.fill(0.0);
                        }
                    });
                }
                while clock::now() < end {
                    std::hint::spin_loop();
                }
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

    /// Four threads, each with its own nodes, spinning for what they
    /// wait for as the engine does: every cycle every node is run, and
    /// the signal crosses the threads whole.
    #[test]
    fn four_threads_run_one_graph() {
        let mut text = String::from("bus mix\npeak meter\nconnect mix -> meter\n");
        for track in 0..8 {
            text.push_str(&format!(
                "impulse t{track} every=1 db=-30\nload t{track}.a ns=20 thread={}\n\
                 load t{track}.b ns=20 thread={}\n\
                 connect t{track} -> t{track}.a:0 -> t{track}.b:0 -> mix\n",
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
