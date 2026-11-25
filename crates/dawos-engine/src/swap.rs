//! One graph in the place of another while the cycle runs (step P1.7e).
//!
//! The live threads never wait for a graph and never free one. There
//! are two places for a graph; the threads run the one that is current,
//! and a new graph is built, outside the real-time path, in the other.
//! Then a cycle is named, a few periods ahead: from that cycle on every
//! thread runs the new graph. All of them change at the same cycle,
//! because they all count the same cycles; none of them changes in the
//! middle of one, because a thread looks once, when its cycle begins,
//! and holds the graph it found until the cycle ends.
//!
//! When every thread is past the cycle that was named, the thread that
//! asked for the change takes the old graph out of its place and frees
//! it: nobody holds it any more.
//!
//! What the new graph keeps of the old one is decided where it is
//! built: `dawos_graph::Running::after`.
//!
//! In this step the change is asked for by the command line, for the
//! tests: two graphs, each taking the place of the other every so many
//! seconds, compiled and built anew every time.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{RwLock, RwLockReadGuard};
use std::time::Duration;

use dawos_audio_io::Format;
use dawos_graph::{Compiled, Running};
use dawos_rt::clock;
use dawos_rt::slot::Slot;
use dawos_rt::stats::Durations;

/// The cycle a change is named for, counted from the latest cycle any
/// thread has begun: time for every thread to see it before it comes.
const CYCLES_AHEAD: u64 = 3;

/// How long the threads are given to get past the cycle of a change
/// before it is called stalled.
const PASS_LIMIT_NS: u64 = 1_000_000_000;

/// The two places, and which of them the threads run.
pub struct Switch {
    places: [RwLock<Option<Running>>; 2],
    /// The place in use until the cycle of the change.
    current: AtomicUsize,
    /// The place in use from that cycle on.
    next: AtomicUsize,
    /// The cycle of the change; `u64::MAX`: none is asked for.
    change_at: AtomicU64,
}

impl Switch {
    /// A switch with `first` as the graph in use.
    #[must_use]
    pub fn new(first: Running) -> Self {
        Self {
            places: [RwLock::new(Some(first)), RwLock::new(None)],
            current: AtomicUsize::new(0),
            next: AtomicUsize::new(0),
            change_at: AtomicU64::new(u64::MAX),
        }
    }

    /// The place of the graph of cycle `cycle`.
    #[inline]
    fn place_for(&self, cycle: u64) -> usize {
        if cycle >= self.change_at.load(Ordering::Acquire) {
            self.next.load(Ordering::Acquire)
        } else {
            self.current.load(Ordering::Acquire)
        }
    }

    /// The graph of cycle `cycle`, held until the guard is dropped, and
    /// the place it is in. For a live thread, when its cycle begins: no
    /// wait, no system call. `None` when the place cannot be read,
    /// which the order of a change rules out.
    #[inline]
    pub fn for_cycle(&self, cycle: u64) -> Option<(usize, RwLockReadGuard<'_, Option<Running>>)> {
        let place = self.place_for(cycle);
        self.places[place]
            .try_read()
            .ok()
            .map(|guard| (place, guard))
    }

    /// The graph in use now. May wait: not for a live thread.
    pub fn in_use(&self) -> Option<RwLockReadGuard<'_, Option<Running>>> {
        self.places[self.current.load(Ordering::Acquire)]
            .read()
            .ok()
    }

    /// Put `graph` in the free place and name cycle `at` for the
    /// change. Returns whether it was taken: not when a change is
    /// already asked for (the graph is then dropped here). May wait:
    /// not for a live thread.
    pub fn offer(&self, graph: Running, at: u64) -> bool {
        if self.change_at.load(Ordering::Acquire) != u64::MAX {
            return false;
        }
        let free = 1 - self.current.load(Ordering::Acquire);
        match self.places[free].write() {
            Ok(mut place) => *place = Some(graph),
            Err(_) => return false,
        }
        // The place first, the cycle last: a thread that sees the cycle
        // finds the graph.
        self.next.store(free, Ordering::Release);
        self.change_at.store(at, Ordering::Release);
        true
    }

    /// The change is made: every thread is past its cycle. The new
    /// graph becomes the one in use, and the old one is taken out of
    /// its place and handed over, to be looked at and freed. `None`
    /// when there is no old graph, or when its place is still held by a
    /// thread in its last cycle of it: ask again.
    pub fn settle(&self) -> Option<Running> {
        if self.change_at.load(Ordering::Acquire) != u64::MAX {
            // The new place first, then no cycle: a thread between the
            // two still finds the new graph.
            self.current
                .store(self.next.load(Ordering::Acquire), Ordering::Release);
            self.change_at.store(u64::MAX, Ordering::Release);
        }
        let old = 1 - self.current.load(Ordering::Acquire);
        self.places[old]
            .try_write()
            .ok()
            .and_then(|mut place| place.take())
    }
}

/// What asks for the changes of a run and remembers what became of
/// them. Owned by the thread that keeps the time of the run.
pub struct Swapper {
    /// The graphs that take each other's place, by their files.
    files: [String; 2],
    threads: usize,
    format: Format,
    load_costs_ps: Option<[u32; 3]>,
    every_ns: u64,
    next_ns: u64,
    /// Which of the two files is in use.
    in_use: usize,
    /// Changes made.
    pub done: u64,
    /// A change the threads did not get past in time, or whose old
    /// graph could not be freed: no change is asked for after it.
    pub stalled: Option<String>,
    /// How long it took to read, compile and build a graph, and from
    /// the moment it was offered to the moment every thread ran it.
    pub prepare: Durations,
    pub passing: Durations,
    /// Nodes that went on with what they were, at the last change.
    pub kept: (usize, usize),
    /// What the graphs that went away had counted: buffers refused.
    pub refused: u64,
    /// The graphs that went away, as they were compiled, with what
    /// their nodes took.
    pub gone: Vec<(Compiled, Vec<dawos_graph::Timed>)>,
    /// The graph in use as it was compiled.
    pub compiled: Compiled,
}

impl Swapper {
    /// A swapper for a run that starts at `start_ns` with `first`,
    /// compiled as `compiled`, and changes to the other file every
    /// `every_s` seconds.
    #[must_use]
    pub fn new(
        files: [String; 2],
        compiled: Compiled,
        threads: usize,
        format: Format,
        load_costs_ps: Option<[u32; 3]>,
        every_s: u64,
        start_ns: u64,
    ) -> Self {
        let every_ns = every_s * 1_000_000_000;
        Self {
            files,
            threads,
            format,
            load_costs_ps,
            every_ns,
            next_ns: start_ns + every_ns,
            in_use: 0,
            done: 0,
            stalled: None,
            prepare: Durations::new(),
            passing: Durations::new(),
            kept: (0, 0),
            refused: 0,
            gone: Vec::new(),
            compiled,
        }
    }

    /// Make the change that is due, if one is. Called again and again
    /// by the thread that keeps the time of the run; it returns when
    /// the change is made, or given up. `stop`: the run is over.
    pub fn tick(&mut self, switch: &Switch, slots: &[Slot], stop: impl Fn() -> bool) {
        if self.stalled.is_some() || clock::now() < self.next_ns {
            return;
        }
        self.next_ns += self.every_ns;
        let other = 1 - self.in_use;

        // ---- Read, compile, build: all the time it takes.
        let began = clock::now();
        let compiled = match crate::graph::load(
            &self.files[other],
            self.threads,
            self.format,
            self.load_costs_ps,
        ) {
            Ok(compiled) => compiled,
            Err(error) => {
                self.stalled = Some(error);
                return;
            }
        };
        let graph = {
            let Some(guard) = switch.in_use() else {
                self.stalled = Some("the graph in use cannot be read".to_string());
                return;
            };
            match guard.as_ref() {
                Some(current) => Running::after(&compiled, current),
                None => Running::new(&compiled),
            }
        };
        let kept = (graph.kept(), graph.len());
        self.prepare.add(clock::now() - began);

        // ---- Offer it for a cycle a little ahead.
        let latest = slots.iter().map(Slot::cycle).max().unwrap_or(0);
        let at = latest + CYCLES_AHEAD;
        let offered = clock::now();
        if !switch.offer(graph, at) {
            self.stalled = Some("a change was still under way".to_string());
            return;
        }

        // ---- Wait until every thread has begun a cycle of the new graph.
        while slots.iter().any(|slot| slot.cycle() < at) {
            if stop() {
                return;
            }
            if clock::now() - offered > PASS_LIMIT_NS {
                self.stalled = Some(format!(
                    "a thread did not get to cycle {at} within {} ms",
                    PASS_LIMIT_NS / 1_000_000
                ));
                return;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        self.passing.add(clock::now() - offered);

        // ---- The old graph: out of its place, looked at, freed. A
        // thread that began its last cycle of the old graph a moment
        // ago may still hold it: a few tries.
        let mut old = None;
        for _ in 0..50 {
            old = switch.settle();
            if old.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        let Some(old) = old else {
            self.stalled = Some("the old graph is still held by a thread".to_string());
            return;
        };
        self.refused += old.refused();
        let before = std::mem::replace(&mut self.compiled, compiled);
        self.gone.push((before, old.timed()));
        // Two descriptions take turns: what the older runs told is in
        // the newer ones of the same file. Keep the last of each.
        if self.gone.len() > 2 {
            self.gone.remove(0);
        }
        self.kept = kept;
        self.in_use = other;
        self.done += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dawos_graph::{compile, parse, Costs, Settings};

    fn running(text: &str) -> Running {
        let settings = Settings {
            threads: 1,
            rate: 48_000,
            frames: 96,
            costs: Costs::default(),
        };
        Running::new(&compile(&parse(text).unwrap(), &settings).unwrap())
    }

    #[test]
    fn every_cycle_finds_one_graph_and_the_change_comes_at_its_cycle() {
        let switch = Switch::new(running("noise a\npeak p\nconnect a -> p\n"));
        let nodes = |cycle: u64| {
            let (place, guard) = switch.for_cycle(cycle).expect("a graph");
            (place, guard.as_ref().expect("a graph in its place").len())
        };
        assert_eq!(nodes(0), (0, 2));
        assert_eq!(nodes(1_000_000), (0, 2));

        // Offered for cycle 10: the cycles before it run the old graph,
        // cycle 10 and those after it the new one.
        let three = running("noise a\ngain g\npeak p\nconnect a -> g -> p\n");
        assert!(switch.offer(three, 10));
        assert_eq!(nodes(9), (0, 2));
        assert_eq!(nodes(10), (1, 3));
        assert_eq!(nodes(11), (1, 3));
        // No second change while one is under way.
        assert!(!switch.offer(running("noise a\n"), 20));

        // A thread still in a cycle of the old graph holds it: it
        // cannot be taken out yet.
        let held = switch.for_cycle(9).expect("the old graph");
        assert!(switch.settle().is_none());
        // The threads are on the new graph for good all the same.
        assert_eq!(nodes(0), (1, 3));
        drop(held);
        assert_eq!(switch.settle().expect("the old graph").len(), 2);
        assert!(switch.settle().is_none(), "it was handed over once");
    }

    #[test]
    fn the_old_graph_is_handed_over_when_nobody_holds_it() {
        let switch = Switch::new(running("noise a\npeak p\nconnect a -> p\n"));
        assert!(switch.settle().is_none(), "no change was asked for");
        assert!(switch.offer(running("noise a\n"), 5));
        let old = switch.settle().expect("the old graph");
        assert_eq!(old.len(), 2);
        assert_eq!(switch.in_use().unwrap().as_ref().unwrap().len(), 1);
        // The place is free again, for the change after.
        assert!(switch.offer(old, 9));
        assert_eq!(switch.settle().expect("the graph before").len(), 1);
    }
}
