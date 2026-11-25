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
//! A thread holds its graph by a counted reference, taken when the
//! cycle begins and let go when it ends: two additions that no other
//! thread waits for. A thread that never ends its cycle (the watchdog
//! has taken its CPU away, ADR 0023) keeps its reference for good; the
//! graph it holds is never freed, and the place it was in is free for
//! the next all the same.
//!
//! Two things ask for a change. The command line, for the tests: two
//! graphs, each taking the place of the other every so many seconds.
//! And the watchdog (step P1.7f): a node it gave up is silence at once,
//! and here a graph without it is compiled and put in use.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
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
    places: [RwLock<Option<Arc<Running>>>; 2],
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
            places: [RwLock::new(Some(Arc::new(first))), RwLock::new(None)],
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

    /// The graph of cycle `cycle` and the place it is in. For a live
    /// thread, when its cycle begins: no wait, no system call. `None`
    /// when the place cannot be read, which the order of a change rules
    /// out.
    #[inline]
    pub fn for_cycle(&self, cycle: u64) -> Option<(usize, Arc<Running>)> {
        let place = self.place_for(cycle);
        let guard = self.places[place].try_read().ok()?;
        guard.as_ref().map(|graph| (place, Arc::clone(graph)))
    }

    /// The graph a thread in cycle `cycle` runs. May wait: for the
    /// watchdog, not for a live thread.
    pub fn of_cycle(&self, cycle: u64) -> Option<Arc<Running>> {
        self.places[self.place_for(cycle)]
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// The graph in use now. May wait: not for a live thread.
    pub fn in_use(&self) -> Option<Arc<Running>> {
        self.places[self.current.load(Ordering::Acquire)]
            .read()
            .ok()
            .and_then(|guard| guard.clone())
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
            Ok(mut place) => *place = Some(Arc::new(graph)),
            Err(_) => return false,
        }
        // The place first, the cycle last: a thread that sees the cycle
        // finds the graph.
        self.next.store(free, Ordering::Release);
        self.change_at.store(at, Ordering::Release);
        true
    }

    /// The change is made: every thread that still runs is past its
    /// cycle. The new graph becomes the one in use, and the old one is
    /// taken out of its place and handed over, to be looked at. It is
    /// freed when the last reference to it goes: here, unless a thread
    /// stuck in a cycle holds one, and then never. `None` when no
    /// change was asked for. May wait: not for a live thread.
    pub fn settle(&self) -> Option<Arc<Running>> {
        if self.change_at.load(Ordering::Acquire) == u64::MAX {
            return None;
        }
        // The new place first, then no cycle: a thread between the two
        // still finds the new graph.
        self.current
            .store(self.next.load(Ordering::Acquire), Ordering::Release);
        self.change_at.store(u64::MAX, Ordering::Release);
        let old = 1 - self.current.load(Ordering::Acquire);
        self.places[old]
            .write()
            .ok()
            .and_then(|mut place| place.take())
    }
}

/// What asks for the changes of a run and remembers what became of
/// them. Owned by the thread that keeps the time of the run.
pub struct Swapper {
    /// The graph of the run by its file; with a second file, the two
    /// take each other's place.
    files: Vec<String>,
    threads: usize,
    format: Format,
    load_costs_ps: Option<[u32; 3]>,
    /// How often the two files take turns; `None`: they do not.
    every_ns: Option<u64>,
    next_ns: u64,
    /// Which of the files is in use.
    in_use: usize,
    /// Changes made.
    pub done: u64,
    /// A change the threads did not get past in time: no change is
    /// asked for after it.
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
    /// The nodes the watchdog gave up, by name: left out of every graph
    /// compiled from then on.
    pub without: Vec<String>,
    /// For each of them, the instant every thread ran a graph without
    /// it.
    pub removed: Vec<(String, u64)>,
}

impl Swapper {
    /// A swapper for a run that starts at `start_ns` with the graph of
    /// `files[0]`, compiled as `compiled`. With a second file and
    /// `every_s`, the two take turns every so many seconds.
    #[must_use]
    pub fn new(
        files: Vec<String>,
        compiled: Compiled,
        threads: usize,
        format: Format,
        load_costs_ps: Option<[u32; 3]>,
        every_s: Option<u64>,
        start_ns: u64,
    ) -> Self {
        let every_ns = every_s
            .filter(|_| files.len() > 1)
            .map(|seconds| seconds * 1_000_000_000);
        Self {
            files,
            threads,
            format,
            load_costs_ps,
            every_ns,
            next_ns: start_ns + every_ns.unwrap_or(0),
            in_use: 0,
            done: 0,
            stalled: None,
            prepare: Durations::new(),
            passing: Durations::new(),
            kept: (0, 0),
            refused: 0,
            gone: Vec::new(),
            compiled,
            without: Vec::new(),
            removed: Vec::new(),
        }
    }

    /// Make the change that is due, if one is. Called again and again
    /// by the thread that keeps the time of the run; it returns when
    /// the change is made, or given up. `stop`: the run is over.
    pub fn tick(&mut self, switch: &Switch, slots: &[Slot], stop: impl Fn() -> bool) {
        if self.stalled.is_some() {
            return;
        }
        // ---- A node the watchdog gave up: a graph without it, now.
        let given_up: Vec<String> = switch
            .in_use()
            .map(|graph| graph.given_up())
            .unwrap_or_default()
            .into_iter()
            .filter(|name| !self.without.contains(name))
            .collect();
        if !given_up.is_empty() {
            self.without.extend(given_up.iter().cloned());
            if self.change(switch, slots, self.in_use, &stop) {
                let now = clock::now();
                self.removed
                    .extend(given_up.into_iter().map(|name| (name, now)));
            }
            return;
        }
        // ---- The turn of the other file.
        let Some(every_ns) = self.every_ns else {
            return;
        };
        if clock::now() < self.next_ns {
            return;
        }
        self.next_ns += every_ns;
        let other = 1 - self.in_use;
        if self.change(switch, slots, other, &stop) {
            self.in_use = other;
        }
    }

    /// Put the graph of file `which`, without the nodes given up, in
    /// the place of the one in use. Returns whether every thread runs
    /// it.
    fn change(
        &mut self,
        switch: &Switch,
        slots: &[Slot],
        which: usize,
        stop: &impl Fn() -> bool,
    ) -> bool {
        // ---- Read, compile, build: all the time it takes.
        let began = clock::now();
        let compiled = match crate::graph::load_without(
            &self.files[which],
            self.threads,
            self.format,
            self.load_costs_ps,
            &self.without,
        ) {
            Ok(compiled) => compiled,
            Err(error) => {
                self.stalled = Some(error);
                return false;
            }
        };
        let Some(current) = switch.in_use() else {
            self.stalled = Some("the graph in use cannot be read".to_string());
            return false;
        };
        let graph = Running::after(&compiled, &current);
        drop(current);
        let kept = (graph.kept(), graph.len());
        self.prepare.add(clock::now() - began);

        // ---- Offer it for a cycle a little ahead.
        let latest = slots.iter().map(Slot::cycle).max().unwrap_or(0);
        let at = latest + CYCLES_AHEAD;
        let offered = clock::now();
        if !switch.offer(graph, at) {
            self.stalled = Some("a change was still under way".to_string());
            return false;
        }

        // ---- Wait until every thread has begun a cycle of the new graph.
        while slots.iter().any(|slot| slot.cycle() < at) {
            if stop() {
                return false;
            }
            if clock::now() - offered > PASS_LIMIT_NS {
                self.stalled = Some(format!(
                    "a thread did not get to cycle {at} within {} ms",
                    PASS_LIMIT_NS / 1_000_000
                ));
                return false;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        self.passing.add(clock::now() - offered);

        // ---- The old graph: out of its place, looked at, let go.
        if let Some(old) = switch.settle() {
            self.refused += old.refused();
            let before = std::mem::replace(&mut self.compiled, compiled);
            self.gone.push((before, old.timed()));
            // Two descriptions take turns: what the older runs told is
            // in the newer ones of the same file. Keep the last of each.
            if self.gone.len() > 2 {
                self.gone.remove(0);
            }
        }
        self.kept = kept;
        self.done += 1;
        true
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
            let (place, graph) = switch.for_cycle(cycle).expect("a graph");
            (place, graph.len())
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

        // A thread still in a cycle of the old graph holds it, and goes
        // on holding it when it is taken out of its place: it is freed
        // when the last one lets go, or never.
        let (_, held) = switch.for_cycle(9).expect("the old graph");
        let old = switch.settle().expect("the old graph");
        assert_eq!(old.len(), 2);
        assert_eq!(Arc::strong_count(&old), 2);
        // The threads are on the new graph for good.
        assert_eq!(nodes(0), (1, 3));
        drop(held);
        assert_eq!(Arc::strong_count(&old), 1);
        assert!(switch.settle().is_none(), "it was handed over once");
    }

    #[test]
    fn the_old_graph_is_handed_over_when_nobody_holds_it() {
        let switch = Switch::new(running("noise a\npeak p\nconnect a -> p\n"));
        assert!(switch.settle().is_none(), "no change was asked for");
        assert!(switch.offer(running("noise a\n"), 5));
        let old = switch.settle().expect("the old graph");
        assert_eq!(old.len(), 2);
        assert_eq!(switch.in_use().unwrap().len(), 1);
        // The place is free again, for the change after.
        assert!(switch.offer(running("noise a\npeak p\nconnect a -> p\n"), 9));
        assert_eq!(switch.settle().expect("the graph before").len(), 1);
    }
}
