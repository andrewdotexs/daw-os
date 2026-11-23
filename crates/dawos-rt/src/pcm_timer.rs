//! The timer the kernel keeps for every sound stream: one tick for each
//! period the card completes (step P1.6b).
//!
//! A live thread that does not hold the sound card sleeps on a timer of
//! its own, bound to the input stream of the card. The kernel puts a
//! tick in the queue of every such timer at the instant a period is
//! complete, the same instant it wakes the thread that holds the card.
//!
//! A tick is an event with a queue of its own, not a state shared with
//! others: nothing another thread does can take it away before this one
//! has seen it. That is what a `poll()` on the stream itself cannot
//! give: there the kernel wakes every sleeper, and each looks again
//! whether a period is there to read; the thread that holds the card
//! reads it some microseconds later, and a sleeper that looks after
//! that goes back to sleep (measured on the 285K: more than half the
//! cycles lost that way).
//!
//! The interface is the one of `/dev/snd/timer`, in
//! `include/uapi/sound/asound.h` of the kernel: three requests to bind
//! the descriptor to a stream, set it to tick at every period and start
//! it, then `poll()` and `read()`.

use std::io;

use crate::sys::{self, Readable};

/// `struct snd_timer_id`.
#[repr(C)]
struct TimerId {
    class: i32,
    subclass: i32,
    card: i32,
    device: i32,
    subdevice: i32,
}

/// `struct snd_timer_select`.
#[repr(C)]
struct Select {
    id: TimerId,
    reserved: [u8; 32],
}

/// `struct snd_timer_params`.
#[repr(C)]
struct Params {
    flags: u32,
    ticks: u32,
    queue_size: u32,
    reserved0: u32,
    filter: u32,
    reserved: [u8; 60],
}

/// `SNDRV_TIMER_CLASS_PCM`: the timers of the sound streams.
const CLASS_PCM: i32 = 3;
/// `SNDRV_TIMER_PSFLG_AUTO`: go on after every tick.
const AUTO: u32 = 1;
/// The shortest queue the kernel gives.
const QUEUE: u32 = 32;

/// The number of a request to a device: the direction, the size of the
/// structure, the letter of the subsystem and the number of the request
/// (`_IOC` in the headers of the kernel).
const fn request(writes: bool, size: usize, number: u32) -> u32 {
    ((writes as u32) << 30) | ((size as u32) << 16) | ((b'T' as u32) << 8) | number
}

/// `SNDRV_TIMER_IOCTL_SELECT`, `_PARAMS` and `_START`.
const SELECT: u32 = request(true, size_of::<Select>(), 0x10);
const PARAMS: u32 = request(true, size_of::<Params>(), 0x12);
const START: u32 = request(false, 0, 0xa0);

/// What a wait on the timer ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// So many periods were completed since the last wait: one, when
    /// the thread keeps up.
    Periods(u32),
    /// The time ran out with no period: the stream is not running.
    Nothing,
    /// The stream is gone, and the timer with it.
    Gone,
}

/// A timer bound to one sound stream. Closed when dropped.
pub struct PcmTimer {
    fd: i32,
}

impl PcmTimer {
    /// The timer of the stream `subdevice` of device `device` of sound
    /// card `card`: the input stream if `capture`, else the output one.
    /// Some system calls: not for the compute phase of a cycle.
    pub fn open(card: u32, device: u32, subdevice: u32, capture: bool) -> io::Result<Self> {
        let number = |value: u32| {
            i32::try_from(value).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
        };
        let timer = Self {
            fd: sys::open_device(c"/dev/snd/timer")?,
        };
        let select = Select {
            id: TimerId {
                class: CLASS_PCM,
                subclass: 0,
                card: number(card)?,
                device: number(device)?,
                // The kernel numbers the two directions of a stream as
                // one after the other: output even, input odd.
                subdevice: number(subdevice)? * 2 + i32::from(capture),
            },
            reserved: [0; 32],
        };
        sys::device_request(timer.fd, SELECT, &select)?;
        let params = Params {
            flags: AUTO,
            ticks: 1,
            queue_size: QUEUE,
            reserved0: 0,
            filter: 0,
            reserved: [0; 60],
        };
        sys::device_request(timer.fd, PARAMS, &params)?;
        sys::device_request_bare(timer.fd, START)?;
        Ok(timer)
    }

    /// Sleep until the stream completes a period, for `timeout_ms` at
    /// the most. Two system calls, a `poll()` and a `read()`: wait
    /// phase only.
    #[must_use]
    pub fn wait(&self, timeout_ms: i32) -> Tick {
        match sys::wait_readable(self.fd, timeout_ms) {
            Readable::NotYet => return Tick::Nothing,
            Readable::Broken => return Tick::Gone,
            Readable::Yes => {}
        }
        // `struct snd_timer_read`: the length of a tick in nanoseconds,
        // then how many of them. Ticks of the same length are added up
        // by the kernel: one record is all there is to read.
        let mut record = [0_u8; 8];
        match sys::read_some(self.fd, &mut record) {
            Ok(8) => Tick::Periods(u32::from_ne_bytes([
                record[4], record[5], record[6], record[7],
            ])),
            Ok(_) => Tick::Nothing,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Tick::Nothing,
            Err(_) => Tick::Gone,
        }
    }
}

impl Drop for PcmTimer {
    fn drop(&mut self) {
        sys::close_descriptor(self.fd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_requests_are_those_of_the_kernel() {
        // The structures have the sizes of the kernel's, and the
        // requests the numbers a trace of alsa-lib shows.
        assert_eq!(size_of::<Select>(), 52);
        assert_eq!(size_of::<Params>(), 80);
        assert_eq!(SELECT, 0x4034_5410);
        assert_eq!(PARAMS, 0x4050_5412);
        assert_eq!(START, 0x54a0);
    }

    #[test]
    fn a_stream_that_is_not_there_has_no_timer() {
        // No machine has a sound card number 31 with a device 31; where
        // there is no /dev/snd/timer at all, the opening fails sooner.
        assert!(PcmTimer::open(31, 31, 0, true).is_err());
    }
}
