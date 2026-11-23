//! One ALSA sound card, opened the way the engine uses it (step P1.4):
//! the `hw` device itself, memory-mapped, capture and playback started
//! together, a buffer of a few periods of one quantum each (three,
//! unless something else is asked: step P1.5).
//!
//! Nothing here knows about threads. Every function may enter the
//! kernel: the live thread calls them in the wait phase of its cycle, or
//! where the comment says that the call stays in user space.

use ::alsa::direct::pcm::Status;
use ::alsa::pcm::{Access, Format as SampleFormat, HwParams, State, TstampType, PCM};
use ::alsa::{Direction, ValueOr};

use crate::Format;

/// How to open a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardConfig {
    /// ALSA name of the capture device, like `hw:USB,0`.
    pub capture: String,
    /// ALSA name of the playback device.
    pub playback: String,
    /// Sample rate and frames per period.
    pub format: Format,
    /// Periods in each buffer: three is the engine's own number
    /// ([`Format::DEFAULT_PERIODS`]); with two the signal slips on a USB
    /// interface, and with more the way round only gets longer.
    pub periods: u32,
}

/// What the driver accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardInfo {
    /// ALSA name of the capture device.
    pub capture: String,
    /// ALSA name of the playback device.
    pub playback: String,
    /// Sample rate in hertz.
    pub rate: u32,
    /// Channels of the capture stream.
    pub inputs: usize,
    /// Channels of the playback stream.
    pub outputs: usize,
    /// Frames per period, the same in both directions.
    pub period: usize,
    /// Frames of the capture buffer.
    pub capture_buffer: usize,
    /// Frames of the playback buffer.
    pub playback_buffer: usize,
    /// Whether the two streams start and stop as one (`snd_pcm_link`).
    pub linked: bool,
    /// Whether the driver stamps every period with the time.
    pub timestamps: bool,
}

/// Why a call on the card did not do its job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardError {
    /// The stream stopped because the engine was late (an xrun): the
    /// card is there, and [`Card::recover`] starts it again.
    Xrun,
    /// Anything else: the card is gone, or it refuses to work.
    Gone(::alsa::Error),
}

/// The two streams as the kernel saw them when an xrun was found: which
/// one ran out, and by how much. Frames; -1 where the kernel did not
/// answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XrunView {
    /// Frames waiting to be read.
    pub capture_avail: i64,
    /// The most frames that ever waited to be read.
    pub capture_avail_max: i64,
    /// Frames of room in the output buffer: the whole buffer means it
    /// ran empty.
    pub playback_avail: i64,
    /// The most room there ever was in the output buffer.
    pub playback_avail_max: i64,
    /// Frames between the last one written and the converters.
    pub playback_delay: i64,
    /// State of the capture stream.
    pub capture_state: &'static str,
    /// State of the playback stream.
    pub playback_state: &'static str,
}

fn state_name(state: State) -> &'static str {
    match state {
        State::Open => "open",
        State::Setup => "setup",
        State::Prepared => "prepared",
        State::Running => "running",
        State::XRun => "xrun",
        State::Draining => "draining",
        State::Paused => "paused",
        State::Suspended => "suspended",
        State::Disconnected => "disconnected",
    }
}

/// What [`Card::wait`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ready {
    /// One period of input is there.
    Period {
        /// The time the kernel stamped on it, in nanoseconds of the
        /// monotonic clock, when the driver gives one.
        stamp: Option<u64>,
        /// Frames of input ready, and frames of output still to play:
        /// both read in the memory shared with the kernel.
        input_ready: u32,
        output_queued: u32,
    },
    /// Nothing came within the time allowed.
    Timeout,
}

/// An open card.
pub struct Card {
    capture: PCM,
    playback: PCM,
    /// The status page of the capture stream, shared with the kernel:
    /// reading it is a read of memory.
    status: Option<Status>,
    info: CardInfo,
    started: bool,
    /// Where the kernel keeps the input stream: sound card, device and
    /// stream of the device. `None`: the driver did not say.
    place: Option<(u32, u32, u32)>,
}

const BYTES_PER_SAMPLE: usize = 4;

fn classify(error: ::alsa::Error) -> CardError {
    // EPIPE: an xrun. ESTRPIPE: the device was suspended. Both are cured
    // by preparing the stream again.
    if error.errno() == libc_epipe() || error.errno() == libc_estrpipe() {
        CardError::Xrun
    } else {
        CardError::Gone(error)
    }
}

// The two error numbers of Linux this file needs: not worth a dependency
// of this crate on libc.
const fn libc_epipe() -> i32 {
    32
}
const fn libc_estrpipe() -> i32 {
    86
}

fn describe(what: &str, device: &str, error: ::alsa::Error) -> String {
    format!("{device}: {what}: {error}")
}

/// Open one direction with the parameters of the engine. Returns the
/// stream, its channels and its buffer size.
fn open_stream(
    device: &str,
    direction: Direction,
    format: Format,
    periods: u32,
) -> Result<(PCM, usize, usize, bool), String> {
    let pcm = PCM::new(device, direction, false)
        .map_err(|error| describe("cannot open the device", device, error))?;
    let frames = i64::from(format.frames);
    let whole = i64::from(periods) * frames;
    let channels;
    {
        let hardware =
            HwParams::any(&pcm).map_err(|error| describe("no parameters", device, error))?;
        let refuse = |what: &str, error| describe(what, device, error);
        // The engine converts nothing behind its own back: the rate is
        // the rate of the converters.
        hardware
            .set_rate_resample(false)
            .map_err(|error| refuse("cannot turn resampling off", error))?;
        hardware
            .set_access(Access::MMapInterleaved)
            .map_err(|error| refuse("no memory-mapped interleaved access", error))?;
        hardware
            .set_format(SampleFormat::s32())
            .map_err(|error| refuse("no 32-bit little-endian samples", error))?;
        hardware
            .set_rate(format.rate, ValueOr::Nearest)
            .map_err(|error| refuse(&format!("no rate of {} Hz", format.rate), error))?;
        // All the channels the device has at this rate: the routing is
        // decided above, not by leaving streams half open.
        let most = hardware
            .get_channels_max()
            .map_err(|error| refuse("no number of channels", error))?;
        hardware
            .set_channels(most)
            .map_err(|error| refuse(&format!("no {most} channels"), error))?;
        hardware
            .set_period_size(frames, ValueOr::Nearest)
            .map_err(|error| refuse(&format!("no period of {frames} frames"), error))?;
        hardware
            .set_buffer_size(whole)
            .map_err(|error| refuse(&format!("no buffer of {whole} frames"), error))?;
        pcm.hw_params(&hardware)
            .map_err(|error| refuse("the parameters were refused", error))?;
        channels = most as usize;
    }

    // What the driver really set: "nearest" may have moved a value.
    let (period, buffer, rate) = {
        let current = pcm
            .hw_params_current()
            .map_err(|error| describe("cannot read the parameters back", device, error))?;
        let read = |value: ::alsa::Result<i64>| {
            value.map_err(|error| describe("cannot read the parameters back", device, error))
        };
        (
            read(current.get_period_size())?,
            read(current.get_buffer_size())?,
            current
                .get_rate()
                .map_err(|error| describe("cannot read the rate back", device, error))?,
        )
    };
    if period != frames || buffer != whole || rate != format.rate {
        return Err(format!(
            "{device}: asked for {} Hz, a period of {frames} frames and a buffer of {}; the driver set {rate} Hz, {period} and {buffer}",
            format.rate, whole
        ));
    }

    let timestamps;
    {
        let software = pcm
            .sw_params_current()
            .map_err(|error| describe("no software parameters", device, error))?;
        let refuse = |what: &str, error| describe(what, device, error);
        software
            .set_avail_min(frames)
            .map_err(|error| refuse("cannot set the wake-up threshold", error))?;
        // Never start by itself: the engine starts both streams, once,
        // when the output buffer is full of silence.
        let boundary = software
            .get_boundary()
            .map_err(|error| refuse("no boundary", error))?;
        software
            .set_start_threshold(boundary)
            .map_err(|error| refuse("cannot set the start threshold", error))?;
        // Stop when the whole buffer went by unserved: that is an xrun.
        software
            .set_stop_threshold(whole)
            .map_err(|error| refuse("cannot set the stop threshold", error))?;
        // The time of every period, on the clock the engine reads.
        timestamps = software.set_tstamp_mode(true).is_ok()
            && software.set_tstamp_type(TstampType::Monotonic).is_ok();
        pcm.sw_params(&software)
            .map_err(|error| refuse("the software parameters were refused", error))?;
    }
    Ok((pcm, channels, buffer as usize, timestamps))
}

impl Card {
    /// Open the two streams, link them and fill the output with silence.
    /// The card does not run yet: see [`Card::start`].
    pub fn open(config: &CardConfig) -> Result<Self, String> {
        let (playback, outputs, playback_buffer, _) = open_stream(
            &config.playback,
            Direction::Playback,
            config.format,
            config.periods,
        )?;
        let (capture, inputs, capture_buffer, timestamps) = open_stream(
            &config.capture,
            Direction::Capture,
            config.format,
            config.periods,
        )?;
        // One trigger for both: they start on the same frame, and an
        // xrun of one stops the other.
        let linked = capture.link(&playback).is_ok();
        let status = if timestamps {
            Status::new(&capture).ok()
        } else {
            None
        };
        // Asked once, here: the answer is made with memory of its own,
        // and that is not for the live thread.
        let place = capture.info().ok().and_then(|info| {
            let card = u32::try_from(info.get_card()).ok()?;
            Some((card, info.get_device(), info.get_subdevice()))
        });
        let mut card = Self {
            capture,
            playback,
            info: CardInfo {
                capture: config.capture.clone(),
                playback: config.playback.clone(),
                rate: config.format.rate,
                inputs,
                outputs,
                period: config.format.frames as usize,
                capture_buffer,
                playback_buffer,
                linked,
                timestamps: status.is_some(),
            },
            status,
            started: false,
            place,
        };
        card.fill_with_silence().map_err(|error| {
            format!(
                "{}: cannot write the first silence: {error:?}",
                config.playback
            )
        })?;
        Ok(card)
    }

    /// What the driver accepted.
    #[must_use]
    pub const fn info(&self) -> &CardInfo {
        &self.info
    }

    /// Whether [`Card::start`] was called since the card was opened or
    /// recovered.
    #[must_use]
    pub const fn started(&self) -> bool {
        self.started
    }

    /// A whole buffer of silence in the output: the distance between
    /// what the engine writes and what the converters play.
    fn fill_with_silence(&mut self) -> Result<(), CardError> {
        let mut left = self.info.playback_buffer;
        let mut rounds = 0;
        while left > 0 && rounds < 4 {
            let written = self
                .playback
                .io_bytes()
                .mmap(left, |bytes| {
                    bytes.fill(0);
                    bytes.len() / (BYTES_PER_SAMPLE * self.info.outputs)
                })
                .map_err(classify)?;
            left = left.saturating_sub(written);
            rounds += 1;
        }
        Ok(())
    }

    /// Start both streams.
    pub fn start(&mut self) -> Result<(), CardError> {
        self.playback.start().map_err(classify)?;
        if self.capture.state() != State::Running {
            self.capture.start().map_err(classify)?;
        }
        self.started = true;
        Ok(())
    }

    /// Sound card, device and stream of the input, as the kernel
    /// numbers them: what a thread needs to open the timer of the stream
    /// and wake with it.
    #[must_use]
    pub const fn place(&self) -> Option<(u32, u32, u32)> {
        self.place
    }

    /// Ask the kernel how the two streams stand: two system calls, for
    /// the moment an xrun is found.
    #[must_use]
    pub fn look(&self) -> XrunView {
        let capture = self.capture.status().ok();
        let playback = self.playback.status().ok();
        XrunView {
            capture_avail: capture.as_ref().map_or(-1, |status| status.get_avail()),
            capture_avail_max: capture.as_ref().map_or(-1, |status| status.get_avail_max()),
            playback_avail: playback.as_ref().map_or(-1, |status| status.get_avail()),
            playback_avail_max: playback
                .as_ref()
                .map_or(-1, |status| status.get_avail_max()),
            playback_delay: playback.as_ref().map_or(-1, |status| status.get_delay()),
            capture_state: state_name(self.capture.state()),
            playback_state: state_name(self.playback.state()),
        }
    }

    /// After an xrun: both streams from the beginning, the output full
    /// of silence again.
    pub fn recover(&mut self) -> Result<(), CardError> {
        self.started = false;
        let gone = |error| CardError::Gone(error);
        self.playback.prepare().map_err(gone)?;
        if self.capture.state() != State::Prepared {
            self.capture.prepare().map_err(gone)?;
        }
        match self.fill_with_silence() {
            Ok(()) => {}
            Err(CardError::Xrun) => {
                return Err(CardError::Gone(::alsa::Error::unsupported("fill")))
            }
            Err(error) => return Err(error),
        }
        match self.start() {
            Ok(()) => Ok(()),
            Err(CardError::Xrun) => Err(CardError::Gone(::alsa::Error::unsupported("start"))),
            Err(error) => Err(error),
        }
    }

    /// Sleep until one period of input is there, or `timeout_ms` went
    /// by. The one blocking call of the cycle.
    pub fn wait(&mut self, timeout_ms: u32) -> Result<Ready, CardError> {
        let period = self.info.period as i64;
        // With the status page mapped the first check stays in user
        // space; then one poll(). A second round only if the kernel woke
        // the thread for less than a period.
        for _ in 0..4 {
            let ready = self.capture.avail_update().map_err(classify)?;
            if ready >= period {
                // How full the output is: its buffer less the room in
                // it. A stream that has just stopped says so at the
                // next exchange of samples, not here.
                let room = self.playback.avail_update().unwrap_or(0);
                let queued = (self.info.playback_buffer as i64 - room).max(0);
                return Ok(Ready::Period {
                    stamp: self.period_time(),
                    input_ready: u32::try_from(ready).unwrap_or(u32::MAX),
                    output_queued: u32::try_from(queued).unwrap_or(u32::MAX),
                });
            }
            if !self.capture.wait(Some(timeout_ms)).map_err(classify)? {
                return Ok(Ready::Timeout);
            }
        }
        Ok(Ready::Timeout)
    }

    /// The time the kernel wrote when it last moved the capture pointer:
    /// the instant the period was complete, as the interrupt saw it.
    fn period_time(&self) -> Option<u64> {
        let stamp = self.status.as_ref()?.htstamp();
        let seconds = u64::try_from(stamp.tv_sec).ok()?;
        let nanoseconds = u64::try_from(stamp.tv_nsec).ok()?;
        let time = seconds
            .checked_mul(1_000_000_000)?
            .checked_add(nanoseconds)?;
        (time != 0).then_some(time)
    }

    /// Copy one period of input, all channels interleaved, into `input`.
    /// No system call: the samples are in memory shared with the kernel.
    pub fn capture_into(&mut self, input: &mut [i32]) -> Result<(), CardError> {
        let channels = self.info.inputs;
        let mut done = 0;
        let mut rounds = 0;
        // The buffer is a ring: a period may come in two pieces.
        while done < input.len() && rounds < 4 {
            let target = &mut input[done..];
            let frames = self
                .capture
                .io_bytes()
                .mmap(target.len() / channels, |bytes| {
                    let mut samples = 0;
                    for (sample, raw) in target.iter_mut().zip(bytes.chunks_exact(BYTES_PER_SAMPLE))
                    {
                        *sample = i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
                        samples += 1;
                    }
                    samples / channels
                })
                .map_err(classify)?;
            done += frames * channels;
            rounds += 1;
        }
        // Less than a period, against what the wait promised: the rest
        // is silence.
        input[done..].fill(0);
        Ok(())
    }

    /// Copy one period of output, all channels interleaved, to the card.
    /// On a USB interface in low-latency mode the commit is a system
    /// call: the driver wants to be told about the new data.
    pub fn deliver_from(&mut self, output: &[i32]) -> Result<usize, CardError> {
        let channels = self.info.outputs;
        let mut done = 0;
        let mut rounds = 0;
        while done < output.len() && rounds < 4 {
            let source = &output[done..];
            let frames = self
                .playback
                .io_bytes()
                .mmap(source.len() / channels, |bytes| {
                    let mut samples = 0;
                    for (sample, raw) in source.iter().zip(bytes.chunks_exact_mut(BYTES_PER_SAMPLE))
                    {
                        raw.copy_from_slice(&sample.to_le_bytes());
                        samples += 1;
                    }
                    samples / channels
                })
                .map_err(classify)?;
            if frames == 0 {
                break;
            }
            done += frames * channels;
            rounds += 1;
        }
        Ok(done / channels)
    }
}
