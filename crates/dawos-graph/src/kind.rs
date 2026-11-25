//! The kinds of node, their parameters, their ports and what they are
//! expected to cost.

use crate::Error;

/// The pitch of a generator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pitch {
    /// In hertz.
    Hz(f64),
    /// As whole cycles in a quantum: the same shape in every period, at
    /// any rate, which is what the measure of slips needs.
    Cycles(u32),
}

/// What a node does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// One capture channel of the sound card (from 1).
    Input {
        /// The channel, from 1.
        channel: u32,
    },
    /// One playback channel of the sound card (from 1).
    Output {
        /// The channel, from 1.
        channel: u32,
    },
    /// Its input, louder or softer.
    Gain {
        /// The gain in decibels.
        db: f64,
    },
    /// The sum of its inputs, as many as are connected.
    Sum,
    /// The sum of its inputs kept in 64 bits while it is made: a bus,
    /// where many signals meet.
    Bus,
    /// Its input, later.
    Delay {
        /// By how many frames.
        frames: u32,
    },
    /// A sine wave.
    Sine {
        /// Its frequency.
        pitch: Pitch,
        /// Its peak in decibels relative to full scale (0 or less).
        db: f64,
    },
    /// One frame at a level, then silence, again and again.
    Impulse {
        /// Frames from one impulse to the next.
        every: u32,
        /// Its level in decibels relative to full scale (0 or less).
        db: f64,
    },
    /// White noise, the same at every run.
    Noise {
        /// Its peak in decibels relative to full scale (0 or less).
        db: f64,
        /// What the sequence starts from.
        seed: u64,
    },
    /// The peak of its input, kept for the report.
    Peak,
    /// A stereo instance of the reference load of requirement R-04 (ADR
    /// 0011): real filtering, as much of it as costs this many
    /// nanoseconds per frame on a live CPU.
    Load {
        /// Nanoseconds per frame it is calibrated for.
        ns_per_frame: u32,
    },
    /// A node that computes nothing: it copies its input to its output
    /// and keeps the CPU busy for so many nanoseconds per frame,
    /// whatever the machine and however the program was built. For the
    /// tests, and to see what the graph itself costs.
    Spin {
        /// Nanoseconds per frame it lasts.
        ns_per_frame: u32,
    },
    /// The filtering of a load with its size said outright, for the
    /// calibration: so many stages and so many trims (see
    /// `dawos_rt::dsp`).
    Cascade {
        /// All-pass sections one after the other.
        stages: u32,
        /// Plain multiplications after them.
        trims: u32,
    },
}

/// How many inputs a kind of node has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inputs {
    /// This many, numbered from 0.
    Fixed(u32),
    /// As many as are connected, in no order.
    Any,
}

/// The classes of the reference load and what each costs per frame (ADR
/// 0011).
const LOAD_CLASSES: [(&str, u32); 3] = [("light", 10), ("medium", 40), ("heavy", 150)];

impl Kind {
    /// The word a description uses for this kind.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            Self::Input { .. } => "input",
            Self::Output { .. } => "output",
            Self::Gain { .. } => "gain",
            Self::Sum => "sum",
            Self::Bus => "bus",
            Self::Delay { .. } => "delay",
            Self::Sine { .. } => "sine",
            Self::Impulse { .. } => "impulse",
            Self::Noise { .. } => "noise",
            Self::Peak => "peak",
            Self::Load { .. } => "load",
            Self::Cascade { .. } => "cascade",
            Self::Spin { .. } => "spin",
        }
    }

    /// The words of every kind.
    pub const WORDS: [&'static str; 13] = [
        "input", "output", "gain", "sum", "bus", "delay", "sine", "impulse", "noise", "peak",
        "load", "cascade", "spin",
    ];

    /// How many inputs it has.
    #[must_use]
    pub fn inputs(&self) -> Inputs {
        match self {
            Self::Input { .. } | Self::Sine { .. } | Self::Impulse { .. } | Self::Noise { .. } => {
                Inputs::Fixed(0)
            }
            Self::Output { .. }
            | Self::Gain { .. }
            | Self::Delay { .. }
            | Self::Peak
            | Self::Spin { .. } => Inputs::Fixed(1),
            Self::Sum | Self::Bus => Inputs::Any,
            Self::Load { .. } | Self::Cascade { .. } => Inputs::Fixed(2),
        }
    }

    /// How many outputs it has.
    #[must_use]
    pub fn outputs(&self) -> u32 {
        match self {
            Self::Output { .. } | Self::Peak => 0,
            Self::Load { .. } | Self::Cascade { .. } => 2,
            _ => 1,
        }
    }

    /// Whether it touches the sound card: such a node runs on the live
    /// thread that holds the card.
    #[must_use]
    pub fn on_the_card(&self) -> bool {
        matches!(self, Self::Input { .. } | Self::Output { .. })
    }

    /// Whether it is an end of the graph: what it receives is used even
    /// though it has no output.
    #[must_use]
    pub fn is_sink(&self) -> bool {
        matches!(self, Self::Output { .. } | Self::Peak)
    }

    /// The kind `word` with the parameters `parameters` (pairs of name
    /// and value, as written), for the node declared on line `line`.
    pub fn from_words(word: &str, parameters: &[(&str, &str)], line: usize) -> Result<Self, Error> {
        let mut given = Given {
            parameters,
            used: vec![false; parameters.len()],
            line,
            word,
        };
        let kind = match word {
            "input" => Self::Input {
                channel: given.channel()?,
            },
            "output" => Self::Output {
                channel: given.channel()?,
            },
            "gain" => Self::Gain {
                db: given.number("db", -200.0, 60.0)?.unwrap_or(0.0),
            },
            "sum" => Self::Sum,
            "bus" => Self::Bus,
            "delay" => Self::Delay {
                frames: given.whole("frames", 1, 1_000_000)?.ok_or_else(|| {
                    Error::new(line, "a delay needs frames=N, how many frames it delays by")
                })?,
            },
            "sine" => {
                let hz = given.number("hz", 1.0, 100_000.0)?;
                let cycles = given.whole("cycles", 1, 1024)?;
                let pitch = match (hz, cycles) {
                    (Some(hz), None) => Pitch::Hz(hz),
                    (None, Some(cycles)) => Pitch::Cycles(cycles),
                    (None, None) => Pitch::Hz(1000.0),
                    (Some(_), Some(_)) => {
                        return Err(Error::new(line, "a sine has hz= or cycles=, not both"))
                    }
                };
                Self::Sine {
                    pitch,
                    db: given.level()?,
                }
            }
            "impulse" => Self::Impulse {
                every: given.whole("every", 1, 100_000_000)?.ok_or_else(|| {
                    Error::new(
                        line,
                        "an impulse needs every=N, frames from one to the next",
                    )
                })?,
                db: given.level()?,
            },
            "noise" => Self::Noise {
                db: given.level()?,
                seed: u64::from(given.whole("seed", 0, u32::MAX)?.unwrap_or(1)),
            },
            "peak" => Self::Peak,
            "load" => {
                let class = given.word("class");
                let ns = given.whole("ns", 1, 100_000)?;
                let ns_per_frame = match (class, ns) {
                    (Some(class), None) => LOAD_CLASSES
                        .iter()
                        .find(|(name, _)| *name == class)
                        .map(|(_, ns)| *ns)
                        .ok_or_else(|| {
                            Error::new(
                                line,
                                format!("no class of load '{class}': light, medium or heavy"),
                            )
                        })?,
                    (None, Some(ns)) => ns,
                    (None, None) => {
                        return Err(Error::new(
                            line,
                            "a load needs class=light, medium or heavy, or ns=N per frame",
                        ))
                    }
                    (Some(_), Some(_)) => {
                        return Err(Error::new(line, "a load has class= or ns=, not both"))
                    }
                };
                Self::Load { ns_per_frame }
            }
            "spin" => Self::Spin {
                ns_per_frame: given
                    .whole("ns", 1, 1_000_000)?
                    .ok_or_else(|| Error::new(line, "a spin needs ns=N, nanoseconds per frame"))?,
            },
            "cascade" => Self::Cascade {
                stages: given.whole("stages", 0, 100_000)?.unwrap_or(0),
                trims: given.whole("trims", 0, 100_000)?.unwrap_or(0),
            },
            _ => {
                return Err(Error::new(
                    line,
                    format!(
                        "no kind of node '{word}': one of {}",
                        Self::WORDS.join(", ")
                    ),
                ))
            }
        };
        given.all_used()?;
        Ok(kind)
    }
}

/// The parameters written on a line, and which of them a kind took.
struct Given<'a> {
    parameters: &'a [(&'a str, &'a str)],
    used: Vec<bool>,
    line: usize,
    word: &'a str,
}

impl<'a> Given<'a> {
    /// The value of the parameter `name`, as written.
    fn word(&mut self, name: &str) -> Option<&'a str> {
        let place = self.parameters.iter().position(|(key, _)| *key == name)?;
        self.used[place] = true;
        Some(self.parameters[place].1)
    }

    /// The parameter `name` as a number from `least` to `most`.
    fn number(&mut self, name: &str, least: f64, most: f64) -> Result<Option<f64>, Error> {
        let Some(written) = self.word(name) else {
            return Ok(None);
        };
        match written.parse::<f64>() {
            Ok(value) if value.is_finite() && (least..=most).contains(&value) => Ok(Some(value)),
            _ => Err(Error::new(
                self.line,
                format!("{name}={written}: a number from {least} to {most}"),
            )),
        }
    }

    /// The parameter `name` as a whole number from `least` to `most`.
    fn whole(&mut self, name: &str, least: u32, most: u32) -> Result<Option<u32>, Error> {
        let Some(written) = self.word(name) else {
            return Ok(None);
        };
        match written.parse::<u32>() {
            Ok(value) if (least..=most).contains(&value) => Ok(Some(value)),
            _ => Err(Error::new(
                self.line,
                format!("{name}={written}: a whole number from {least} to {most}"),
            )),
        }
    }

    /// The channel of the card a node stands for.
    fn channel(&mut self) -> Result<u32, Error> {
        self.whole("channel", 1, 256)?.ok_or_else(|| {
            Error::new(
                self.line,
                format!("an {} needs channel=N, the channel of the card", self.word),
            )
        })
    }

    /// The level of a generator: 12 dB below full scale unless said.
    fn level(&mut self) -> Result<f64, Error> {
        Ok(self.number("db", -200.0, 0.0)?.unwrap_or(-12.0))
    }

    /// Every parameter written must have been taken.
    fn all_used(&self) -> Result<(), Error> {
        match self.used.iter().position(|used| !used) {
            None => Ok(()),
            Some(place) => Err(Error::new(
                self.line,
                format!(
                    "a {} has no parameter '{}'",
                    self.word, self.parameters[place].0
                ),
            )),
        }
    }
}

/// What each kind of node is expected to cost, to give the nodes to the
/// threads before anything is measured, and what a load is made of to
/// cost what it declares.
///
/// The three figures of the load are the calibration of ADR 0011: they
/// say how much filtering makes 10, 40 and 150 ns per frame on a live
/// CPU of the reference machine. Until step P1.7d has measured them
/// there, they are the figures of `dawos-dspbench` for a stage (0.4174
/// ns for each of four instances in a vector, experiment E-06) and
/// estimates for the rest, as are the costs of the other kinds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Costs {
    /// Nanoseconds every node costs whatever it does: finding it,
    /// counting it done.
    pub node_ns: f64,
    /// Nanoseconds lost when a node takes its input from a node of
    /// another thread: the buffer is in the cache of another CPU.
    pub other_thread_ns: f64,
    /// Nanoseconds per frame of an input or an output of the card: a
    /// conversion between integers and floating point.
    pub card_ns: f64,
    /// Nanoseconds per frame of a gain.
    pub gain_ns: f64,
    /// Nanoseconds per frame and per input of a sum.
    pub sum_ns: f64,
    /// Nanoseconds per frame and per input of a bus.
    pub bus_ns: f64,
    /// Nanoseconds per frame of a delay line.
    pub delay_ns: f64,
    /// Nanoseconds per frame of a sine.
    pub sine_ns: f64,
    /// Nanoseconds per frame of an impulse.
    pub impulse_ns: f64,
    /// Nanoseconds per frame of noise.
    pub noise_ns: f64,
    /// Nanoseconds per frame of a peak meter.
    pub peak_ns: f64,
    /// Nanoseconds per frame of a load before any filtering: from two
    /// channels to the eight values of a vector and back, and what
    /// every node costs, spread over the frames of the quantum the
    /// calibration is made at (192 at 96 kHz, the reference session of
    /// requirement R-04).
    pub load_frame_ns: f64,
    /// Nanoseconds per frame of one stage of a load.
    pub stage_ns: f64,
    /// Nanoseconds per frame of one trim of a load.
    pub trim_ns: f64,
}

impl Default for Costs {
    fn default() -> Self {
        Self {
            node_ns: 50.0,
            other_thread_ns: 1000.0,
            card_ns: 1.0,
            gain_ns: 0.3,
            sum_ns: 0.3,
            bus_ns: 0.6,
            delay_ns: 0.5,
            sine_ns: 4.0,
            impulse_ns: 0.3,
            noise_ns: 1.5,
            peak_ns: 0.5,
            load_frame_ns: 2.2,
            stage_ns: 1.67,
            trim_ns: 0.34,
        }
    }
}

impl Costs {
    /// How many stages and how many trims make a load of `ns_per_frame`
    /// nanoseconds per frame: as many stages as fit, and trims for what
    /// is left, to the nearest. At least one of the two, so that a load
    /// always computes something.
    #[must_use]
    pub fn load_shape(&self, ns_per_frame: u32) -> (u32, u32) {
        let room = (f64::from(ns_per_frame) - self.load_frame_ns).max(0.0);
        let stages = (room / self.stage_ns).floor();
        let trims = ((room - stages * self.stage_ns) / self.trim_ns).round();
        // Counts of some thousands at the most: nothing is lost.
        let (stages, trims) = (stages as u32, trims as u32);
        if stages + trims == 0 {
            (0, 1)
        } else {
            (stages, trims)
        }
    }

    /// Nanoseconds per frame of a cascade of `stages` and `trims`.
    #[must_use]
    pub fn cascade_ns(&self, stages: u32, trims: u32) -> f64 {
        self.load_frame_ns + f64::from(stages) * self.stage_ns + f64::from(trims) * self.trim_ns
    }

    /// Nanoseconds a node of kind `kind` with `inputs` inputs connected
    /// is expected to take over `frames` frames. Never zero.
    #[must_use]
    pub fn of(&self, kind: &Kind, inputs: usize, frames: u32) -> u64 {
        let per_frame = match kind {
            Kind::Input { .. } | Kind::Output { .. } => self.card_ns,
            Kind::Gain { .. } => self.gain_ns,
            Kind::Sum => self.sum_ns * inputs.max(1) as f64,
            Kind::Bus => self.bus_ns * inputs.max(1) as f64,
            Kind::Delay { .. } => self.delay_ns,
            Kind::Sine { .. } => self.sine_ns,
            Kind::Impulse { .. } => self.impulse_ns,
            Kind::Noise { .. } => self.noise_ns,
            Kind::Peak => self.peak_ns,
            Kind::Load { ns_per_frame } => {
                let (stages, trims) = self.load_shape(*ns_per_frame);
                self.cascade_ns(stages, trims)
            }
            Kind::Cascade { stages, trims } => self.cascade_ns(*stages, *trims),
            Kind::Spin { ns_per_frame } => f64::from(*ns_per_frame),
        };
        // A load costs what it declares, all of it: what every node
        // costs is inside its cost per frame, as the calibration
        // measures it at the quantum of the reference session.
        let fixed = match kind {
            Kind::Load { .. } | Kind::Cascade { .. } => 0.0,
            _ => self.node_ns,
        };
        let total = fixed + per_frame * f64::from(frames);
        // Costs are small positive numbers: the conversion cannot lose
        // anything that matters.
        (total.round() as u64).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A word, its parameters, and what must be said of them.
    type Wrong = (
        &'static str,
        &'static [(&'static str, &'static str)],
        &'static str,
    );

    #[test]
    fn every_kind_is_read_from_its_word() {
        let cases: [(&str, &[(&str, &str)]); 13] = [
            ("input", &[("channel", "3")]),
            ("output", &[("channel", "5")]),
            ("gain", &[("db", "-6")]),
            ("sum", &[]),
            ("bus", &[]),
            ("delay", &[("frames", "48")]),
            ("sine", &[("cycles", "3")]),
            ("impulse", &[("every", "4800")]),
            ("noise", &[]),
            ("peak", &[]),
            ("load", &[("class", "medium")]),
            ("cascade", &[("stages", "64")]),
            ("spin", &[("ns", "100")]),
        ];
        for (word, parameters) in cases {
            let kind = Kind::from_words(word, parameters, 1).unwrap();
            assert_eq!(kind.word(), word);
        }
        assert_eq!(cases.len(), Kind::WORDS.len());
    }

    #[test]
    fn the_classes_of_load_cost_what_the_decision_says() {
        for (class, ns) in [("light", 10), ("medium", 40), ("heavy", 150)] {
            let kind = Kind::from_words("load", &[("class", class)], 1).unwrap();
            assert_eq!(kind, Kind::Load { ns_per_frame: ns });
        }
        assert_eq!(
            Kind::from_words("load", &[("ns", "33")], 1).unwrap(),
            Kind::Load { ns_per_frame: 33 }
        );
    }

    #[test]
    fn what_is_wrong_is_said_with_the_line() {
        let cases: [Wrong; 8] = [
            ("gian", &[], "no kind of node 'gian'"),
            ("input", &[], "needs channel=N"),
            ("input", &[("channel", "0")], "a whole number from 1 to 256"),
            ("gain", &[("db", "loud")], "a number from -200 to 60"),
            ("gain", &[("level", "3")], "has no parameter 'level'"),
            ("sine", &[("hz", "440"), ("cycles", "3")], "not both"),
            ("load", &[], "needs class="),
            ("load", &[("class", "huge")], "no class of load 'huge'"),
        ];
        for (word, parameters, expected) in cases {
            let error = Kind::from_words(word, parameters, 7).unwrap_err();
            assert_eq!(error.line, 7);
            assert!(error.message.contains(expected), "{}", error.message);
        }
    }

    #[test]
    fn ports_by_kind() {
        let load = Kind::Load { ns_per_frame: 10 };
        assert_eq!(load.inputs(), Inputs::Fixed(2));
        assert_eq!(load.outputs(), 2);
        let cascade = Kind::Cascade {
            stages: 3,
            trims: 0,
        };
        assert_eq!(cascade.inputs(), Inputs::Fixed(2));
        assert_eq!(cascade.outputs(), 2);
        assert_eq!(Kind::Sum.inputs(), Inputs::Any);
        assert_eq!(Kind::Peak.outputs(), 0);
        assert!(Kind::Peak.is_sink());
        assert!(Kind::Input { channel: 1 }.on_the_card());
        assert!(!Kind::Sum.on_the_card());
    }

    #[test]
    fn a_load_is_made_to_cost_what_it_declares() {
        let costs = Costs::default();
        // Whatever the calibration, the classes of ADR 0011 come within
        // half a trim of what they declare.
        for ns in [10, 33, 40, 150, 2000] {
            let (stages, trims) = costs.load_shape(ns);
            let made = costs.cascade_ns(stages, trims);
            assert!(
                (made - f64::from(ns)).abs() <= costs.trim_ns / 2.0 + 1e-9,
                "{ns} ns: {stages} stages and {trims} trims make {made}"
            );
            assert!(f64::from(trims) * costs.trim_ns < costs.stage_ns + costs.trim_ns);
        }
        // A load too light for a stage is trims alone, and never nothing.
        assert_eq!(costs.load_shape(3), (0, 2));
        assert_eq!(costs.load_shape(1), (0, 1));
        // With another calibration, another shape.
        let slower = Costs {
            stage_ns: 3.0,
            ..costs
        };
        assert!(slower.load_shape(150).0 < costs.load_shape(150).0);
    }

    #[test]
    fn a_load_costs_its_class_times_the_frames() {
        let costs = Costs::default();
        let heavy = Kind::Load { ns_per_frame: 150 };
        // 96 frames at 150 ns, to a trim.
        let cost = costs.of(&heavy, 2, 96) as f64;
        assert!((cost - 14_400.0).abs() < 96.0 * costs.trim_ns, "{cost}");
        // A cascade costs by what it is made of.
        let cascade = Kind::Cascade {
            stages: 10,
            trims: 0,
        };
        assert_eq!(
            costs.of(&cascade, 2, 100),
            (100.0 * (costs.load_frame_ns + 10.0 * costs.stage_ns)).round() as u64
        );
        // A sum costs by the input.
        assert!(costs.of(&Kind::Sum, 8, 96) > costs.of(&Kind::Sum, 2, 96));
        assert!(costs.of(&Kind::Peak, 0, 0) >= 1);
    }
}
