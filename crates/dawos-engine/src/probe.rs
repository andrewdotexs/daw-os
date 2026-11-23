//! Two instruments for the way round of a sound card, from an output
//! back to an input (step P1.5):
//!
//! - [`Counter`]: every sample played carries its own number. Where the
//!   way round keeps the samples as they are (a loop inside the
//!   interface, or a card made of software), every sample that comes
//!   back says exactly how long it took: the length of the way round to
//!   the sample, every slip with its size and its sign, and every sample
//!   that came back changed. The numbers are written with a key of the
//!   run, so that samples of another run, left in the interface, are not
//!   taken for the ones of this run.
//! - [`Bursts`]: short bursts of noise, or single impulses, for a way
//!   round that goes through the converters and a cable, where no sample
//!   comes back as it left. The input after each burst is kept, and
//!   compared with the burst once the run is over: the delay at which
//!   they match best is the length of the way round.
//!
//! Both work on one frame at a time, on the live thread: no allocation
//! and no system call after they are built. The comparing of the bursts
//! is heavy work and is done after the run, by whoever asks for it.

/// The number a sample carries wraps at this many samples: a third of a
/// second at 192 kHz, far more than any way round.
const COUNTER_WRAP: i64 = 1 << 16;

/// A sample carries its number 8 bits up: the 16 low bits of a 24-bit
/// sample, 42 dB below full scale at the most.
const COUNTER_SHIFT: u32 = 8;

/// Samples in a row that must agree on a delay before it is believed.
const LOCK_RUN: u32 = 64;

/// How many slips are kept one by one.
const SLIPS_KEPT: usize = 64;

/// Samples of noise in a burst.
const BURST_LENGTH: usize = 1024;

/// What the counter found.
#[derive(Clone, Debug, PartialEq)]
pub struct Counted {
    /// The input that was checked, from 0.
    pub input: usize,
    /// When the delay was first found, in seconds from the first sample;
    /// `None`: never (the way round changes the samples, or is silent).
    pub locked_at: Option<f64>,
    /// The length of the way round when first found, and at the end, in
    /// samples.
    pub first: i64,
    pub last: i64,
    /// Changes of the length after the warm-up: how many, and the
    /// largest (without its sign).
    pub slips: u32,
    pub largest_slip: i64,
    /// The same during the warm-up.
    pub early_slips: u32,
    pub largest_early_slip: i64,
    /// The first changes: seconds from the first sample, and by how many
    /// samples the way round got longer (negative: shorter).
    pub each_slip: Vec<(f64, i64)>,
    /// Samples that, once the delay was known, did not come back where
    /// they belonged: silence, changed samples, and those around a slip.
    pub out_of_place: u64,
    /// Samples that came back where they belonged.
    pub in_place: u64,
}

/// Plays a count on one output and reads it back on one input.
pub struct Counter {
    output: usize,
    /// The number of a sample is multiplied by this before it is played,
    /// and what comes back by the other: an odd number and its inverse,
    /// modulo the wrap. A run with another key reads these samples as
    /// numbers in no order, and never finds a delay in them.
    multiplier: i64,
    inverse: i64,
    rate: f64,
    warm_up_frames: u64,
    locked: Option<i64>,
    candidate: i64,
    run: u32,
    counted: Counted,
}

impl Counter {
    /// A counter from `output` to `input`, at `rate` frames a second;
    /// what happens in the first `warm_up_seconds` is counted apart.
    /// `key` should differ from one run to the next: an interface can
    /// give back, when its streams start, the last samples of the run
    /// before (seen at 192 kHz on the Scarlett 18i20, step P1.5).
    #[must_use]
    pub fn new(output: usize, input: usize, rate: u32, warm_up_seconds: u32, key: u16) -> Self {
        let multiplier = i64::from(key | 1);
        // Newton's steps double the bits that are right: three are
        // right to begin with (every odd number is its own inverse
        // modulo 8), sixteen are needed.
        let mut inverse = multiplier;
        for _ in 0..3 {
            inverse = (inverse * (2 - multiplier * inverse)) & (COUNTER_WRAP - 1);
        }
        Self {
            output,
            multiplier,
            inverse,
            rate: f64::from(rate),
            warm_up_frames: u64::from(warm_up_seconds) * u64::from(rate),
            locked: None,
            candidate: -1,
            run: 0,
            counted: Counted {
                input,
                locked_at: None,
                first: 0,
                last: 0,
                slips: 0,
                largest_slip: 0,
                early_slips: 0,
                largest_early_slip: 0,
                each_slip: Vec::with_capacity(SLIPS_KEPT),
                out_of_place: 0,
                in_place: 0,
            },
        }
    }

    /// The output that carries the count.
    #[must_use]
    pub const fn output(&self) -> usize {
        self.output
    }

    /// The input that is checked.
    #[must_use]
    pub const fn input(&self) -> usize {
        self.counted.input
    }

    /// The sample to play at frame `number`.
    #[must_use]
    pub const fn sample(&self, number: u64) -> i32 {
        let carried = ((number as i64 & (COUNTER_WRAP - 1)) * self.multiplier) & (COUNTER_WRAP - 1);
        (carried << COUNTER_SHIFT) as i32
    }

    /// The sample that came back at frame `number`.
    pub fn read(&mut self, number: u64, sample: i32) {
        let carried = i64::from(sample >> COUNTER_SHIFT) & (COUNTER_WRAP - 1);
        let played = (carried * self.inverse) & (COUNTER_WRAP - 1);
        let delay = (number as i64 - played) & (COUNTER_WRAP - 1);
        if self.locked == Some(delay) {
            self.counted.in_place += 1;
            self.run = 0;
            return;
        }
        if self.locked.is_some() {
            self.counted.out_of_place += 1;
        }
        if delay == self.candidate {
            self.run += 1;
        } else {
            self.candidate = delay;
            self.run = 1;
        }
        if self.run < LOCK_RUN {
            return;
        }
        // Enough samples in a row agree: this is the delay now.
        let second = number as f64 / self.rate;
        match self.locked {
            None => {
                self.counted.locked_at = Some(second);
                self.counted.first = delay;
            }
            Some(before) => {
                // The shorter way round the wrap is the change.
                let mut change = delay - before;
                if change > COUNTER_WRAP / 2 {
                    change -= COUNTER_WRAP;
                } else if change < -COUNTER_WRAP / 2 {
                    change += COUNTER_WRAP;
                }
                if number < self.warm_up_frames {
                    self.counted.early_slips += 1;
                    self.counted.largest_early_slip =
                        self.counted.largest_early_slip.max(change.abs());
                } else {
                    self.counted.slips += 1;
                    self.counted.largest_slip = self.counted.largest_slip.max(change.abs());
                }
                if self.counted.each_slip.len() < self.counted.each_slip.capacity() {
                    self.counted.each_slip.push((second, change));
                }
            }
        }
        self.locked = Some(delay);
        self.counted.last = delay;
        self.run = 0;
    }

    /// What was found.
    #[must_use]
    pub fn counted(&self) -> Counted {
        self.counted.clone()
    }
}

/// What a burst is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstKind {
    /// A thousand samples of noise: the delay is found by comparing,
    /// which a little noise on the way does not disturb.
    Noise,
    /// One sample: what comes back is the answer of the way round to an
    /// impulse, and its highest point is the delay.
    Impulse,
}

/// What the bursts found, once compared.
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    /// Output and input, from 0.
    pub output: usize,
    pub input: usize,
    /// The length of the way round found for every burst, in samples,
    /// with the fraction a curve through the three best delays gives.
    pub each_burst: Vec<f64>,
    /// How well the best delay matched, burst by burst: 1 is a perfect
    /// copy, 0 no likeness at all. For an impulse: the height of the
    /// peak against full scale.
    pub each_match: Vec<f64>,
}

/// Plays bursts on one output and keeps what follows on one input.
pub struct Bursts {
    output: usize,
    input: usize,
    kind: BurstKind,
    /// The burst as it is played.
    signal: Vec<i32>,
    /// Frames from the beginning of a burst to the beginning of the
    /// next, and frames of input kept after each beginning.
    every: usize,
    window: usize,
    /// The first burst begins here: the card has started by then.
    first: u64,
    /// What came in after each burst, one after the other.
    kept: Vec<i32>,
    bursts: usize,
}

impl Bursts {
    /// Bursts from `output` to `input` four times a second, for a run of
    /// `seconds` at `rate` frames a second, played at `amplitude` of
    /// full scale. The way round is looked for up to `longest` frames.
    #[must_use]
    pub fn new(
        output: usize,
        input: usize,
        kind: BurstKind,
        rate: u32,
        seconds: u64,
        amplitude: f64,
        longest: usize,
    ) -> Self {
        let level = (amplitude * 2_147_483_648.0).clamp(0.0, 2_147_483_647.0) as i32;
        let signal = match kind {
            BurstKind::Impulse => vec![level],
            BurstKind::Noise => {
                // A fixed sequence of +level and -level: the same at
                // every run, and unlike itself at any other delay.
                let mut state = 0x9E37_79B9_7F4A_7C15_u64;
                (0..BURST_LENGTH)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        if state & 1 == 0 {
                            level
                        } else {
                            -level
                        }
                    })
                    .collect()
            }
        };
        let rate = rate as usize;
        // Room for the longest way round and for the burst itself; and
        // never so much that a burst runs into the next.
        let every = rate / 4;
        let window = (signal.len() + longest).min(every);
        // None in the first half second; then four a second.
        let bursts = (seconds as usize * 4).saturating_sub(3).min(2000);
        Self {
            output,
            input,
            kind,
            signal,
            every,
            window,
            first: (rate / 2) as u64,
            kept: vec![0; bursts * window],
            bursts,
        }
    }

    /// The output that carries the bursts.
    #[must_use]
    pub const fn output(&self) -> usize {
        self.output
    }

    /// The input that is kept.
    #[must_use]
    pub const fn input(&self) -> usize {
        self.input
    }

    /// Where frame `number` falls: which burst, and how far into it.
    fn place(&self, number: u64) -> Option<(usize, usize)> {
        let since = number.checked_sub(self.first)? as usize;
        let burst = since / self.every;
        (burst < self.bursts).then_some((burst, since % self.every))
    }

    /// The sample to play at frame `number`.
    #[must_use]
    pub fn sample(&self, number: u64) -> i32 {
        match self.place(number) {
            Some((_, offset)) if offset < self.signal.len() => self.signal[offset],
            _ => 0,
        }
    }

    /// The sample that came in at frame `number`.
    pub fn read(&mut self, number: u64, sample: i32) {
        if let Some((burst, offset)) = self.place(number) {
            if offset < self.window {
                self.kept[burst * self.window + offset] = sample;
            }
        }
    }

    /// Compare every burst with what came in after it. Heavy: for after
    /// the run, off the live CPUs. Only bursts wholly played count.
    #[must_use]
    pub fn timed(&self, frames_done: u64) -> Timed {
        let length = self.signal.len();
        let delays = self.window - length;
        let mut each_burst = Vec::new();
        let mut each_match = Vec::new();
        let signal_energy: f64 = self
            .signal
            .iter()
            .map(|&sample| f64::from(sample) * f64::from(sample))
            .sum();
        for burst in 0..self.bursts {
            let end = self.first + (burst * self.every + self.window) as u64;
            if end > frames_done {
                break;
            }
            let kept = &self.kept[burst * self.window..(burst + 1) * self.window];
            // How alike the burst and the input are, at every delay.
            let likeness: Vec<f64> = (0..delays)
                .map(|delay| {
                    self.signal
                        .iter()
                        .zip(&kept[delay..delay + length])
                        .map(|(&played, &heard)| f64::from(played) * f64::from(heard))
                        .sum()
                })
                .collect();
            let Some((best, &peak)) = likeness
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            else {
                continue;
            };
            // A parabola through the best delay and its two neighbours:
            // where its top is, between samples.
            let mut fraction = 0.0;
            if best > 0 && best + 1 < delays {
                let (before, after) = (likeness[best - 1].abs(), likeness[best + 1].abs());
                let curve = before - 2.0 * peak.abs() + after;
                if curve < 0.0 {
                    fraction = 0.5 * (before - after) / curve;
                }
            }
            each_burst.push(best as f64 + fraction);
            each_match.push(match self.kind {
                BurstKind::Impulse => f64::from(kept[best]).abs() / 2_147_483_648.0,
                BurstKind::Noise => {
                    let heard_energy: f64 = kept[best..best + length]
                        .iter()
                        .map(|&sample| f64::from(sample) * f64::from(sample))
                        .sum();
                    if heard_energy > 0.0 {
                        peak.abs() / (signal_energy * heard_energy).sqrt()
                    } else {
                        0.0
                    }
                }
            });
        }
        Timed {
            output: self.output,
            input: self.input,
            each_burst,
            each_match,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// A key like any other.
    const KEY: u16 = 0x9e37;

    /// Play the counter into a line `delay` samples long, with changes
    /// of length on the way, and read it back.
    fn count(frames: u64, delay: usize, changes: &[(u64, i64)], warm_up: u32) -> Counted {
        let mut counter = Counter::new(0, 0, RATE, warm_up, KEY);
        let mut line: std::collections::VecDeque<i32> = std::iter::repeat_n(0, delay).collect();
        for number in 0..frames {
            for &(at, change) in changes {
                if at == number {
                    if change > 0 {
                        line.extend(std::iter::repeat_n(0, change as usize));
                    } else {
                        drop(line.drain(..(-change) as usize));
                    }
                }
            }
            line.push_back(counter.sample(number));
            let back = line.pop_front().unwrap_or(0);
            counter.read(number, back);
        }
        counter.counted()
    }

    #[test]
    fn the_counter_finds_the_length_to_the_sample() {
        let counted = count(48_000, 300, &[], 0);
        assert_eq!(counted.first, 300);
        assert_eq!(counted.last, 300);
        assert_eq!(counted.slips, 0);
        assert_eq!(counted.out_of_place, 0);
        // Found as soon as the first samples are back.
        assert!(counted.locked_at.expect("found") < 0.01);
    }

    #[test]
    fn the_counter_finds_every_slip_with_its_sign() {
        let counted = count(
            4 * 48_000,
            300,
            &[(48_000, 1), (96_000, 94), (144_000, -3)],
            0,
        );
        assert_eq!(counted.first, 300);
        assert_eq!(counted.last, 392);
        assert_eq!(counted.slips, 3);
        assert_eq!(counted.largest_slip, 94);
        let sizes: Vec<i64> = counted.each_slip.iter().map(|slip| slip.1).collect();
        assert_eq!(sizes, vec![1, 94, -3]);
        assert!((counted.each_slip[1].0 - 2.0).abs() < 0.01);
    }

    #[test]
    fn slips_in_the_warm_up_are_counted_apart() {
        let counted = count(4 * 48_000, 300, &[(48_000, 23), (144_000, 1)], 2);
        assert_eq!(counted.early_slips, 1);
        assert_eq!(counted.largest_early_slip, 23);
        assert_eq!(counted.slips, 1);
        assert_eq!(counted.largest_slip, 1);
    }

    #[test]
    fn a_way_that_changes_the_samples_is_never_found() {
        let mut counter = Counter::new(0, 0, RATE, 0, KEY);
        for number in 0..48_000_u64 {
            // Half the level: the numbers no longer mean anything.
            counter.read(number, counter.sample(number) / 2 + 77);
        }
        let counted = counter.counted();
        assert_eq!(counted.locked_at, None);
        assert_eq!(counted.in_place, 0);
    }

    #[test]
    fn every_key_reads_its_own_count() {
        for key in [0, 1, 2, 0x00ff, 0x9e37, 0xfffe, 0xffff] {
            let mut counter = Counter::new(0, 0, RATE, 0, key);
            for number in 0..200_000_u64 {
                // Straight back, 7 samples late.
                counter.read(number + 7, counter.sample(number));
            }
            let counted = counter.counted();
            assert_eq!(counted.first, 7, "key {key}");
            assert_eq!(counted.out_of_place, 0, "key {key}");
        }
    }

    #[test]
    fn the_samples_of_another_run_are_not_taken_for_these() {
        // What the Scarlett did at 192 kHz: the streams start, and the
        // first thing to come back is the end of the run before.
        let before = Counter::new(0, 0, RATE, 0, 0x1234);
        let mut counter = Counter::new(0, 0, RATE, 0, KEY);
        let mut number = 0_u64;
        for old in 50_000..50_500_u64 {
            counter.read(number, before.sample(old));
            number += 1;
        }
        assert_eq!(counter.counted().locked_at, None);
        // Then its own samples, 300 late.
        for _ in 0..10_000 {
            counter.read(number, counter.sample(number - 300));
            number += 1;
        }
        let counted = counter.counted();
        assert_eq!(counted.first, 300);
        assert_eq!(counted.slips, 0);
    }

    /// Play bursts into a line and read them back, quieter and with a
    /// little of something else added.
    fn time(kind: BurstKind, delay: usize) -> Timed {
        let mut bursts = Bursts::new(2, 2, kind, RATE, 3, 0.25, 960);
        let mut line: std::collections::VecDeque<i32> = std::iter::repeat_n(0, delay).collect();
        let frames = 3 * u64::from(RATE);
        let mut other = 1_u32;
        for number in 0..frames {
            line.push_back(bursts.sample(number));
            let back = line.pop_front().unwrap_or(0);
            other = other.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let hum = (other >> 12) as i32 - (1 << 19);
            bursts.read(number, back / 3 + hum);
        }
        bursts.timed(frames)
    }

    #[test]
    fn bursts_of_noise_find_the_length() {
        let timed = time(BurstKind::Noise, 437);
        assert_eq!(timed.each_burst.len(), 9);
        for (length, likeness) in timed.each_burst.iter().zip(&timed.each_match) {
            assert!((length - 437.0).abs() < 0.05, "{timed:?}");
            assert!(*likeness > 0.99, "{timed:?}");
        }
    }

    #[test]
    fn an_impulse_finds_the_length() {
        let timed = time(BurstKind::Impulse, 437);
        assert_eq!(timed.each_burst.len(), 9);
        for length in &timed.each_burst {
            assert!((length - 437.0).abs() < 0.5, "{timed:?}");
        }
    }

    #[test]
    fn nothing_is_played_before_the_card_has_started() {
        let bursts = Bursts::new(0, 0, BurstKind::Noise, RATE, 2, 0.25, 960);
        assert!((0..u64::from(RATE) / 2).all(|number| bursts.sample(number) == 0));
        assert_ne!(bursts.sample(u64::from(RATE) / 2), 0);
    }
}
