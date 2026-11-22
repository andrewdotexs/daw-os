//! What the engine does with sound before it has a graph (step P1.4):
//! a test tone on one output, inputs copied to outputs, and a measure of
//! what comes in on chosen inputs.
//!
//! It runs on the first live thread, once per cycle, on the samples of
//! the card as they are: 32-bit integers. Nothing here allocates after
//! [`Patch::new`], and nothing calls the system.

use std::f64::consts::TAU;

use crate::probe::{BurstKind, Bursts, Counted, Counter, Timed};

/// Full scale of a 32-bit sample.
const FULL_SCALE: f64 = 2_147_483_648.0;

/// The tone is followed quantum by quantum only above this amplitude
/// (-60 dB): below it there is no phase to speak of.
const TONE_FLOOR: f64 = 1.0e-3;

/// A change of the way round counts as a slip from a quarter of a
/// sample: the noise of a measure on a clean tone is a hundred times
/// smaller.
const SLIP_SAMPLES: f64 = 0.25;

/// How many slips are kept one by one, per input.
const SLIPS_KEPT: usize = 64;

/// What to play, copy and measure. Channels are counted from 0 here and
/// from 1 on the command line.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PatchConfig {
    /// Output that carries the test tone.
    pub tone_output: Option<usize>,
    /// Frequency of the tone in hertz, and its peak level in dB below
    /// full scale (a negative number).
    pub tone_hz: f64,
    pub tone_db: f64,
    /// Inputs copied to outputs, sample for sample: (input, output).
    pub routes: Vec<(usize, usize)>,
    /// Inputs to measure.
    pub listen: Vec<usize>,
    /// How many seconds to keep one by one, from the first (0: none).
    pub keep_seconds: usize,
    /// Seconds at the beginning that are not judged: the interface is
    /// still settling. What happens there is counted apart. The first
    /// second is never judged.
    pub warm_up_seconds: u32,
    /// A count played on an output and checked on an input, sample for
    /// sample: (output, input). For a way round that keeps the samples
    /// as they are.
    pub count: Option<(usize, usize)>,
    /// The key of the count: it should differ from one run to the next
    /// (see [`Counter::new`]).
    pub count_key: u16,
    /// Bursts played on an output and timed on an input: (output,
    /// input, what they are made of). For a way round through the
    /// converters.
    pub bursts: Option<(usize, usize, BurstKind)>,
    /// How long the run lasts, in seconds: the room for the bursts.
    pub seconds: u64,
}

/// A sudden change in the time the tone takes to come back: samples were
/// lost or made up somewhere on the way round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slip {
    /// When, in seconds from the first sample.
    pub second: f64,
    /// By how many samples the way round got longer (negative:
    /// shorter). Known only up to a whole cycle of the tone.
    pub samples: f64,
}

/// One second of one input, summed up.
#[derive(Clone, Copy, Debug, Default)]
struct Block {
    squares: f64,
    in_phase: f64,
    quadrature: f64,
    peak: f64,
}

/// What was heard on one input over the run. Levels in dB relative to
/// full scale; the first second and the last, incomplete one are left
/// out.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    /// The input, from 0.
    pub channel: usize,
    /// Whole seconds measured.
    pub seconds: u32,
    /// The highest sample.
    pub peak_db: f64,
    /// The effective level of the quietest and of the loudest second.
    pub level_min_db: f64,
    pub level_max_db: f64,
    /// The peak level of the test tone alone, in the second where it was
    /// weakest and in the one where it was strongest.
    pub tone_min_db: f64,
    pub tone_max_db: f64,
    /// The share of the power that was the tone, in the worst second: 1
    /// is a pure tone.
    pub share_min: f64,
    /// Second by second, from the first one: effective level, level of
    /// the tone, share. As many as were asked to be kept.
    pub each_second: Vec<(f64, f64, f64)>,
    /// Slips after the first second: how many, the largest (in samples,
    /// without its sign), and the first ones.
    pub slip_count: u32,
    pub largest_slip: f64,
    pub slips: Vec<Slip>,
    /// Slips during the warm-up: how many, and the largest.
    pub early_slip_count: u32,
    pub largest_early_slip: f64,
}

struct Listener {
    channel: usize,
    block: Block,
    heard: Heard,
    /// The quantum being read: the two sums that give the phase of the
    /// tone.
    in_phase: f64,
    quadrature: f64,
    /// The phase of the tone in the quantum before, once it has been
    /// there for two quanta.
    last_phase: Option<f64>,
    steady: u32,
    /// A slip being followed: it shows in two quanta, the one it falls
    /// in and the next.
    slipping: f64,
    slip_started: Option<f64>,
}

/// The state of the patch.
pub struct Patch {
    inputs: usize,
    outputs: usize,
    frames: usize,
    tone_output: Option<usize>,
    amplitude: f64,
    routes: Vec<(usize, usize)>,
    listeners: Vec<Listener>,
    counter: Option<Counter>,
    bursts: Option<Bursts>,
    /// The oscillator: a point on the unit circle, turned by a fixed
    /// angle at every sample. Its two coordinates are the tone and the
    /// reference the measure needs.
    cosine: f64,
    sine: f64,
    step_cosine: f64,
    step_sine: f64,
    /// Samples in a second, and how far into the second we are.
    block_frames: usize,
    position: usize,
    blocks: u32,
    /// Seconds at the beginning that are not judged: one at least.
    warm_up: u32,
    /// Radians of the tone per sample, the rate, and frames done.
    angle: f64,
    rate: f64,
    done: u64,
    input: Vec<i32>,
    output: Vec<i32>,
}

fn decibels(ratio: f64) -> f64 {
    if ratio > 0.0 {
        20.0 * ratio.log10()
    } else {
        f64::NEG_INFINITY
    }
}

impl Patch {
    /// A patch for a card with these channels, at this rate and quantum.
    pub fn new(
        config: &PatchConfig,
        inputs: usize,
        outputs: usize,
        frames: usize,
        rate: u32,
    ) -> Result<Self, String> {
        let input_exists = |channel: usize, what: &str| {
            if channel < inputs {
                Ok(())
            } else {
                Err(format!(
                    "{what}: input {} does not exist: the card has {inputs} at this rate",
                    channel + 1
                ))
            }
        };
        let output_exists = |channel: usize, what: &str| {
            if channel < outputs {
                Ok(())
            } else {
                Err(format!(
                    "{what}: output {} does not exist: the card has {outputs} at this rate",
                    channel + 1
                ))
            }
        };
        if let Some(channel) = config.tone_output {
            output_exists(channel, "--tone")?;
        }
        for &(input, output) in &config.routes {
            input_exists(input, "--route")?;
            output_exists(output, "--route")?;
            if config.tone_output == Some(output) {
                return Err(format!(
                    "--route: output {} already carries the tone",
                    output + 1
                ));
            }
        }
        for &channel in &config.listen {
            input_exists(channel, "--listen")?;
        }
        // One thing per output.
        let mut taken: Vec<usize> = config.routes.iter().map(|route| route.1).collect();
        taken.extend(config.tone_output);
        if let Some((output, input)) = config.count {
            output_exists(output, "--count")?;
            input_exists(input, "--count")?;
            if taken.contains(&output) {
                return Err(format!("--count: output {} is already in use", output + 1));
            }
            taken.push(output);
        }
        if let Some((output, input, _)) = config.bursts {
            output_exists(output, "--burst")?;
            input_exists(input, "--burst")?;
            if taken.contains(&output) {
                return Err(format!("--burst: output {} is already in use", output + 1));
            }
        }
        let angle = TAU * config.tone_hz / f64::from(rate);
        Ok(Self {
            inputs,
            outputs,
            frames,
            tone_output: config.tone_output,
            amplitude: 10.0_f64.powf(config.tone_db / 20.0),
            routes: config.routes.clone(),
            counter: config.count.map(|(output, input)| {
                Counter::new(
                    output,
                    input,
                    rate,
                    config.warm_up_seconds,
                    config.count_key,
                )
            }),
            bursts: config.bursts.map(|(output, input, kind)| {
                Bursts::new(
                    output,
                    input,
                    kind,
                    rate,
                    config.seconds,
                    10.0_f64.powf(config.tone_db / 20.0),
                    // Twenty milliseconds, or the largest buffers the
                    // card can be given (eight periods) and one more.
                    (rate as usize / 50).max(9 * frames),
                )
            }),
            listeners: config
                .listen
                .iter()
                .map(|&channel| Listener {
                    channel,
                    block: Block::default(),
                    heard: Heard {
                        channel,
                        seconds: 0,
                        peak_db: f64::NEG_INFINITY,
                        level_min_db: f64::INFINITY,
                        level_max_db: f64::NEG_INFINITY,
                        tone_min_db: f64::INFINITY,
                        tone_max_db: f64::NEG_INFINITY,
                        share_min: f64::INFINITY,
                        // Room for all of them now: nothing grows later.
                        each_second: Vec::with_capacity(config.keep_seconds),
                        slip_count: 0,
                        largest_slip: 0.0,
                        slips: Vec::with_capacity(SLIPS_KEPT),
                        early_slip_count: 0,
                        largest_early_slip: 0.0,
                    },
                    in_phase: 0.0,
                    quadrature: 0.0,
                    last_phase: None,
                    steady: 0,
                    slipping: 0.0,
                    slip_started: None,
                })
                .collect(),
            cosine: 1.0,
            sine: 0.0,
            step_cosine: angle.cos(),
            step_sine: angle.sin(),
            block_frames: rate as usize,
            position: 0,
            blocks: 0,
            warm_up: config.warm_up_seconds.max(1),
            angle,
            rate: f64::from(rate),
            done: 0,
            input: vec![0; inputs * frames],
            output: vec![0; outputs * frames],
        })
    }

    /// Where the back end writes the input of the cycle.
    pub fn input_mut(&mut self) -> &mut [i32] {
        &mut self.input
    }

    /// What goes to the back end at the end of the cycle.
    #[must_use]
    pub fn output(&self) -> &[i32] {
        &self.output
    }

    /// One quantum: from the input just read to the output to deliver.
    pub fn process(&mut self) {
        self.output.fill(0);
        for frame in 0..self.frames {
            let input = &self.input[frame * self.inputs..(frame + 1) * self.inputs];
            let output = &mut self.output[frame * self.outputs..(frame + 1) * self.outputs];
            for &(from, to) in &self.routes {
                output[to] = input[from];
            }
            if let Some(channel) = self.tone_output {
                // The clamp keeps the cast inside a 32-bit sample at
                // full scale.
                let sample =
                    (self.amplitude * self.sine * FULL_SCALE).clamp(-FULL_SCALE, FULL_SCALE - 1.0);
                output[channel] = sample as i32;
            }
            let number = self.done + frame as u64;
            if let Some(counter) = &mut self.counter {
                output[counter.output()] = counter.sample(number);
                counter.read(number, input[counter.input()]);
            }
            if let Some(bursts) = &mut self.bursts {
                output[bursts.output()] = bursts.sample(number);
                bursts.read(number, input[bursts.input()]);
            }
            for listener in &mut self.listeners {
                let sample = f64::from(input[listener.channel]) / FULL_SCALE;
                let block = &mut listener.block;
                block.squares += sample * sample;
                block.in_phase += sample * self.cosine;
                block.quadrature += sample * self.sine;
                block.peak = block.peak.max(sample.abs());
                listener.in_phase += sample * self.cosine;
                listener.quadrature += sample * self.sine;
            }

            // Turn the oscillator by one sample.
            let cosine = self.cosine * self.step_cosine - self.sine * self.step_sine;
            let sine = self.sine * self.step_cosine + self.cosine * self.step_sine;
            self.cosine = cosine;
            self.sine = sine;

            self.position += 1;
            if self.position == self.block_frames {
                self.close_block();
            }
        }
        self.done += self.frames as u64;
        self.follow_the_tone();
    }

    /// A quantum is over: where is the phase of the tone, on every input
    /// listened to, against the quantum before? A tone that comes back
    /// after the same number of samples keeps its phase; one sample more
    /// on the way turns it by the angle of one sample.
    ///
    /// Exact when the tone has a whole number of cycles in a quantum
    /// (1125 Hz with the quanta of 2.667 ms); with any other tone the
    /// phase of a clean signal wobbles, and slips are reported that are
    /// not there.
    fn follow_the_tone(&mut self) {
        let count = self.frames as f64;
        let now = self.done as f64 / self.rate;
        let counted = self.blocks >= self.warm_up;
        for listener in &mut self.listeners {
            let in_phase = std::mem::take(&mut listener.in_phase);
            let quadrature = std::mem::take(&mut listener.quadrature);
            let amplitude = 2.0 * in_phase.hypot(quadrature) / count;
            if amplitude < TONE_FLOOR {
                listener.last_phase = None;
                listener.steady = 0;
                listener.slipping = 0.0;
                listener.slip_started = None;
                continue;
            }
            // With the tone sin(a) coming back as sin(a - delay), the
            // two sums are -sin(delay) and cos(delay): this angle grows
            // with the delay.
            let phase = quadrature.atan2(in_phase);
            let mut moved = 0.0;
            if let (Some(last), true) = (listener.last_phase, listener.steady >= 2) {
                moved = phase - last;
                if moved > std::f64::consts::PI {
                    moved -= TAU;
                } else if moved < -std::f64::consts::PI {
                    moved += TAU;
                }
            }
            if moved.abs() > SLIP_SAMPLES * self.angle {
                listener.slipping += moved;
                listener.slip_started.get_or_insert(now);
            } else if let Some(second) = listener.slip_started.take() {
                // The little that is left in this quantum belongs to
                // the slip too.
                let samples = (std::mem::take(&mut listener.slipping) + moved) / self.angle;
                let heard = &mut listener.heard;
                if samples.abs() < SLIP_SAMPLES {
                    // Too small to be told from the noise of the measure.
                } else if counted {
                    heard.slip_count += 1;
                    heard.largest_slip = heard.largest_slip.max(samples.abs());
                } else {
                    heard.early_slip_count += 1;
                    heard.largest_early_slip = heard.largest_early_slip.max(samples.abs());
                }
                // Kept one by one all the same, warm-up included: the
                // time says which is which.
                if samples.abs() >= SLIP_SAMPLES && heard.slips.len() < heard.slips.capacity() {
                    heard.slips.push(Slip { second, samples });
                }
            }
            listener.last_phase = Some(phase);
            listener.steady += 1;
        }
    }

    /// A second is over: sum it up, and start the next.
    fn close_block(&mut self) {
        let count = self.block_frames as f64;
        for listener in &mut self.listeners {
            let block = std::mem::take(&mut listener.block);
            let power = block.squares / count;
            let tone = 2.0 * block.in_phase.hypot(block.quadrature) / count;
            let share = if power > 0.0 {
                (tone * tone / 2.0 / power).min(1.0)
            } else {
                0.0
            };
            let heard = &mut listener.heard;
            if heard.each_second.len() < heard.each_second.capacity() {
                heard
                    .each_second
                    .push((decibels(power.sqrt()), decibels(tone), share));
            }
            // The warm-up (the first second at least) holds the start
            // of the card: left out of the summary.
            if self.blocks < self.warm_up {
                continue;
            }
            heard.seconds += 1;
            heard.peak_db = heard.peak_db.max(decibels(block.peak));
            let level = decibels(power.sqrt());
            heard.level_min_db = heard.level_min_db.min(level);
            heard.level_max_db = heard.level_max_db.max(level);
            let tone = decibels(tone);
            heard.tone_min_db = heard.tone_min_db.min(tone);
            heard.tone_max_db = heard.tone_max_db.max(tone);
            heard.share_min = heard.share_min.min(share);
        }
        self.blocks += 1;
        self.position = 0;
        // Rounding makes the point drift off the circle, very slowly:
        // put it back once a second.
        let length = self.cosine.hypot(self.sine);
        self.cosine /= length;
        self.sine /= length;
    }

    /// What the count found, when one was asked for.
    #[must_use]
    pub fn counted(&self) -> Option<Counted> {
        self.counter.as_ref().map(Counter::counted)
    }

    /// What the bursts found, when they were asked for. Heavy: compares
    /// every burst with what came in after it.
    #[must_use]
    pub fn timed(&self) -> Option<Timed> {
        self.bursts.as_ref().map(|bursts| bursts.timed(self.done))
    }

    /// What was heard on every input listened to.
    #[must_use]
    pub fn heard(&self) -> Vec<Heard> {
        self.listeners
            .iter()
            .map(|listener| listener.heard.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const FRAMES: usize = 128;

    fn config() -> PatchConfig {
        PatchConfig {
            tone_output: Some(2),
            tone_hz: 1000.0,
            tone_db: -12.0,
            routes: vec![(2, 4)],
            listen: vec![2, 4, 0],
            keep_seconds: 2,
            warm_up_seconds: 0,
            count: None,
            count_key: 0x5a5a,
            bursts: None,
            seconds: 0,
        }
    }

    /// Run the patch with a cable from every output to the input of the
    /// same number, one quantum late: what a loopback card does.
    fn run_with_cables(patch: &mut Patch, cycles: usize, channels: usize) {
        let mut cable = vec![0; channels * FRAMES];
        for _ in 0..cycles {
            patch.input_mut().copy_from_slice(&cable);
            patch.process();
            cable.copy_from_slice(patch.output());
        }
    }

    #[test]
    fn the_tone_comes_back_and_goes_through() {
        let mut patch = Patch::new(&config(), 8, 8, FRAMES, RATE).expect("a valid patch");
        // Four seconds and a bit.
        run_with_cables(&mut patch, 4 * 375 + 10, 8);
        let heard = patch.heard();
        // Input 3 has the tone from output 3; input 5 has it through the
        // route 3 -> 5; input 1 has nothing.
        for channel in [0, 1] {
            let heard = &heard[channel];
            assert_eq!(heard.seconds, 3);
            assert!((heard.tone_min_db + 12.0).abs() < 0.01, "{heard:?}");
            assert!((heard.tone_max_db + 12.0).abs() < 0.01, "{heard:?}");
            assert!(heard.share_min > 0.999, "{heard:?}");
            assert!((heard.peak_db + 12.0).abs() < 0.01, "{heard:?}");
            // A sine: effective level 3 dB below its peak.
            assert!((heard.level_min_db + 15.01).abs() < 0.01, "{heard:?}");
            // Two seconds kept, the first one included.
            assert_eq!(heard.each_second.len(), 2);
            assert!((heard.each_second[1].1 + 12.0).abs() < 0.01, "{heard:?}");
        }
        let silent = &heard[2];
        assert_eq!(silent.seconds, 3);
        assert_eq!(silent.peak_db, f64::NEG_INFINITY);
        assert_eq!(silent.share_min, 0.0);
    }

    #[test]
    fn a_change_of_delay_is_a_slip() {
        // A tone with a whole number of cycles per quantum, and a cable
        // a little longer than a quantum; then three samples longer,
        // then five samples shorter than that.
        let config = PatchConfig {
            tone_output: Some(0),
            tone_hz: 1125.0,
            tone_db: -12.0,
            listen: vec![0],
            ..PatchConfig::default()
        };
        let mut patch = Patch::new(&config, 1, 1, FRAMES, RATE).expect("a valid patch");
        let mut line: std::collections::VecDeque<i32> =
            std::iter::repeat_n(0, FRAMES + 16).collect();
        for cycle in 0..5 * 375 {
            match cycle {
                // Two seconds in: three samples made up.
                750 => line.extend([0, 0, 0]),
                // Three and a half seconds in: five samples lost.
                1312 => drop(line.drain(..5)),
                _ => {}
            }
            for sample in patch.input_mut() {
                *sample = line.pop_front().unwrap_or(0);
            }
            patch.process();
            line.extend(patch.output().iter().copied());
        }
        let heard = &patch.heard()[0];
        assert_eq!(heard.slip_count, 2, "{:?}", heard.slips);
        assert!((heard.largest_slip - 5.0).abs() < 0.05, "{heard:?}");
        assert!(
            (heard.slips[0].samples - 3.0).abs() < 0.05,
            "{:?}",
            heard.slips
        );
        assert!(
            (heard.slips[0].second - 2.0).abs() < 0.01,
            "{:?}",
            heard.slips
        );
        assert!(
            (heard.slips[1].samples + 5.0).abs() < 0.05,
            "{:?}",
            heard.slips
        );
        assert!(
            (heard.slips[1].second - 3.5).abs() < 0.01,
            "{:?}",
            heard.slips
        );
    }

    #[test]
    fn a_slip_in_the_warm_up_is_counted_apart() {
        let config = PatchConfig {
            tone_output: Some(0),
            tone_hz: 1125.0,
            tone_db: -12.0,
            listen: vec![0],
            warm_up_seconds: 3,
            ..PatchConfig::default()
        };
        let mut patch = Patch::new(&config, 1, 1, FRAMES, RATE).expect("a valid patch");
        let mut line: std::collections::VecDeque<i32> =
            std::iter::repeat_n(0, FRAMES + 16).collect();
        for cycle in 0..6 * 375 {
            match cycle {
                // Two seconds in, during the warm-up: three samples
                // made up.
                750 => line.extend([0, 0, 0]),
                // Four and a half seconds in, after it: one sample lost.
                1687 => drop(line.drain(..1)),
                _ => {}
            }
            for sample in patch.input_mut() {
                *sample = line.pop_front().unwrap_or(0);
            }
            patch.process();
            line.extend(patch.output().iter().copied());
        }
        let heard = &patch.heard()[0];
        assert_eq!(heard.early_slip_count, 1, "{heard:?}");
        assert!((heard.largest_early_slip - 3.0).abs() < 0.05, "{heard:?}");
        assert_eq!(heard.slip_count, 1, "{heard:?}");
        assert!((heard.largest_slip - 1.0).abs() < 0.05, "{heard:?}");
        // Both are kept, with their time; and only the seconds after
        // the warm-up are summed up.
        assert_eq!(heard.slips.len(), 2);
        assert_eq!(heard.seconds, 3);
    }

    #[test]
    fn a_steady_tone_does_not_slip() {
        let mut config = config();
        config.tone_hz = 1125.0;
        let mut patch = Patch::new(&config, 8, 8, FRAMES, RATE).expect("a valid patch");
        run_with_cables(&mut patch, 4 * 375, 8);
        for heard in patch.heard() {
            assert_eq!(heard.slip_count, 0, "{:?}", heard.slips);
        }
    }

    #[test]
    fn the_count_and_the_bursts_go_round_with_the_tone() {
        let config = PatchConfig {
            tone_output: Some(0),
            tone_hz: 1125.0,
            tone_db: -12.0,
            listen: vec![0],
            count: Some((4, 4)),
            bursts: Some((2, 2, BurstKind::Noise)),
            seconds: 3,
            ..PatchConfig::default()
        };
        let mut patch = Patch::new(&config, 8, 8, FRAMES, RATE).expect("a valid patch");
        run_with_cables(&mut patch, 3 * 375, 8);
        // The cable is one quantum long.
        let counted = patch.counted().expect("a count");
        assert_eq!(counted.first, FRAMES as i64);
        assert_eq!(counted.slips, 0);
        let timed = patch.timed().expect("bursts");
        assert!(!timed.each_burst.is_empty());
        for length in &timed.each_burst {
            assert!((length - FRAMES as f64).abs() < 0.01, "{timed:?}");
        }
        assert_eq!(patch.heard()[0].slip_count, 0);
    }

    #[test]
    fn two_things_on_one_output_are_refused() {
        let mut config = config();
        config.count = Some((2, 0));
        let error = Patch::new(&config, 8, 8, FRAMES, RATE)
            .err()
            .expect("an error");
        assert!(
            error.contains("--count: output 3 is already in use"),
            "'{error}'"
        );
        let mut config = PatchConfig {
            count: Some((1, 1)),
            bursts: Some((1, 1, BurstKind::Impulse)),
            ..PatchConfig::default()
        };
        config.tone_hz = 1000.0;
        let error = Patch::new(&config, 8, 8, FRAMES, RATE)
            .err()
            .expect("an error");
        assert!(
            error.contains("--burst: output 2 is already in use"),
            "'{error}'"
        );
    }

    #[test]
    fn a_copy_is_exact() {
        let config = PatchConfig {
            routes: vec![(0, 1), (1, 0)],
            tone_hz: 1000.0,
            ..PatchConfig::default()
        };
        let mut patch = Patch::new(&config, 2, 2, 4, RATE).expect("a valid patch");
        patch
            .input_mut()
            .copy_from_slice(&[1, -2, i32::MAX, i32::MIN, 5, 6, 7, 8]);
        patch.process();
        assert_eq!(patch.output(), &[-2, 1, i32::MIN, i32::MAX, 6, 5, 8, 7]);
    }

    #[test]
    fn other_noise_lowers_the_share() {
        let config = PatchConfig {
            tone_hz: 1000.0,
            listen: vec![0],
            ..PatchConfig::default()
        };
        let mut patch = Patch::new(&config, 1, 1, FRAMES, RATE).expect("a valid patch");
        // A tone of another frequency: all of its power is "not the
        // tone".
        let mut phase = 0.0_f64;
        for _ in 0..3 * 375 + 10 {
            for sample in patch.input_mut() {
                *sample = (0.25 * phase.sin() * FULL_SCALE) as i32;
                phase += TAU * 3000.0 / f64::from(RATE);
            }
            patch.process();
        }
        let heard = &patch.heard()[0];
        assert!(heard.share_min < 0.001, "{heard:?}");
        assert!((heard.level_min_db + 15.05).abs() < 0.1, "{heard:?}");
    }

    #[test]
    fn channels_that_do_not_exist_are_refused() {
        let wrong = |change: fn(&mut PatchConfig), expected: &str| {
            let mut config = config();
            change(&mut config);
            let error = Patch::new(&config, 8, 8, FRAMES, RATE)
                .err()
                .expect("an error");
            assert!(error.contains(expected), "'{error}'");
        };
        wrong(|config| config.tone_output = Some(8), "--tone: output 9");
        wrong(|config| config.routes = vec![(8, 0)], "--route: input 9");
        wrong(|config| config.routes = vec![(0, 9)], "--route: output 10");
        wrong(
            |config| config.routes = vec![(0, 2)],
            "already carries the tone",
        );
        wrong(|config| config.listen = vec![12], "--listen: input 13");
    }
}
