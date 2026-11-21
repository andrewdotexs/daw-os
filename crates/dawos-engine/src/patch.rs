//! What the engine does with sound before it has a graph (step P1.4):
//! a test tone on one output, inputs copied to outputs, and a measure of
//! what comes in on chosen inputs.
//!
//! It runs on the first live thread, once per cycle, on the samples of
//! the card as they are: 32-bit integers. Nothing here allocates after
//! [`Patch::new`], and nothing calls the system.

use std::f64::consts::TAU;

/// Full scale of a 32-bit sample.
const FULL_SCALE: f64 = 2_147_483_648.0;

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
}

struct Listener {
    channel: usize,
    block: Block,
    heard: Heard,
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
        let angle = TAU * config.tone_hz / f64::from(rate);
        Ok(Self {
            inputs,
            outputs,
            frames,
            tone_output: config.tone_output,
            amplitude: 10.0_f64.powf(config.tone_db / 20.0),
            routes: config.routes.clone(),
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
                    },
                })
                .collect(),
            cosine: 1.0,
            sine: 0.0,
            step_cosine: angle.cos(),
            step_sine: angle.sin(),
            block_frames: rate as usize,
            position: 0,
            blocks: 0,
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
            for listener in &mut self.listeners {
                let sample = f64::from(input[listener.channel]) / FULL_SCALE;
                let block = &mut listener.block;
                block.squares += sample * sample;
                block.in_phase += sample * self.cosine;
                block.quadrature += sample * self.sine;
                block.peak = block.peak.max(sample.abs());
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
    }

    /// A second is over: sum it up, and start the next.
    fn close_block(&mut self) {
        let count = self.block_frames as f64;
        for listener in &mut self.listeners {
            let block = std::mem::take(&mut listener.block);
            // The first second holds the start of the card: left out.
            if self.blocks == 0 {
                continue;
            }
            let power = block.squares / count;
            let tone = 2.0 * block.in_phase.hypot(block.quadrature) / count;
            let share = if power > 0.0 {
                (tone * tone / 2.0 / power).min(1.0)
            } else {
                0.0
            };
            let heard = &mut listener.heard;
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
        }
        let silent = &heard[2];
        assert_eq!(silent.seconds, 3);
        assert_eq!(silent.peak_db, f64::NEG_INFINITY);
        assert_eq!(silent.share_min, 0.0);
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
