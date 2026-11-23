//! A ring of fixed records between one writer and one reader, for what a
//! live thread measures at every cycle (step P1.6).
//!
//! The writer never waits and never enters the kernel: when the ring is
//! full the record is dropped and counted. The reader is a thread
//! outside the live CPUs that empties the ring some tens of times a
//! second.
//!
//! A record is [`WORDS`] numbers of 64 bits. Every number is an atomic of
//! its own: no `unsafe` code, and on x86-64 a store to one is a plain
//! move. What keeps a record whole is the order of the two counters: the
//! reader looks only at records the writer has already counted, and the
//! writer writes only into records the reader has already counted.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Numbers in one record.
pub const WORDS: usize = 8;

/// One record.
pub type Record = [u64; WORDS];

/// What the two ends share.
struct Shared {
    cells: Box<[AtomicU64]>,
    /// Records the ring holds, a power of two.
    capacity: u64,
    /// Records written since the beginning; only the writer adds.
    written: AtomicU64,
    /// Records read since the beginning; only the reader adds.
    read: AtomicU64,
    /// Records that found the ring full.
    dropped: AtomicU64,
}

impl Shared {
    /// Where record number `count` begins in `cells`.
    fn place(&self, count: u64) -> usize {
        (count & (self.capacity - 1)) as usize * WORDS
    }
}

/// The end that writes: one thread, the live one.
pub struct Writer {
    shared: Arc<Shared>,
}

/// The end that reads: one thread, outside the live CPUs.
pub struct Reader {
    shared: Arc<Shared>,
}

/// A ring with room for at least `records` records (the next power of
/// two, and no less than two). Allocates: call it before the cycle
/// starts.
#[must_use]
pub fn ring(records: usize) -> (Writer, Reader) {
    let capacity = records.max(2).next_power_of_two();
    let cells: Vec<AtomicU64> = (0..capacity * WORDS).map(|_| AtomicU64::new(0)).collect();
    let shared = Arc::new(Shared {
        cells: cells.into_boxed_slice(),
        capacity: capacity as u64,
        written: AtomicU64::new(0),
        read: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
    });
    (
        Writer {
            shared: Arc::clone(&shared),
        },
        Reader { shared },
    )
}

impl Writer {
    /// Add a record. `false`: the ring was full, and the record was
    /// counted as dropped. No allocation, no system call, no waiting.
    #[inline]
    pub fn push(&mut self, record: &Record) -> bool {
        let shared = &*self.shared;
        // Only this end changes `written`.
        let written = shared.written.load(Ordering::Relaxed);
        if written - shared.read.load(Ordering::Acquire) >= shared.capacity {
            shared.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let place = shared.place(written);
        for (cell, &word) in shared.cells[place..place + WORDS].iter().zip(record) {
            cell.store(word, Ordering::Relaxed);
        }
        // From here the reader may look at it.
        shared.written.store(written + 1, Ordering::Release);
        true
    }
}

impl Reader {
    /// Take the oldest record, if there is one.
    pub fn pop(&mut self) -> Option<Record> {
        let shared = &*self.shared;
        // Only this end changes `read`.
        let read = shared.read.load(Ordering::Relaxed);
        if read == shared.written.load(Ordering::Acquire) {
            return None;
        }
        let place = shared.place(read);
        let mut record = [0; WORDS];
        for (word, cell) in record.iter_mut().zip(&shared.cells[place..place + WORDS]) {
            *word = cell.load(Ordering::Relaxed);
        }
        // From here the writer may use the place again.
        shared.read.store(read + 1, Ordering::Release);
        Some(record)
    }

    /// Records waiting to be read.
    #[must_use]
    pub fn waiting(&self) -> u64 {
        self.shared.written.load(Ordering::Acquire) - self.shared.read.load(Ordering::Relaxed)
    }

    /// Records the writer could not add because the ring was full.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }

    /// Records the ring holds.
    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.shared.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(number: u64) -> Record {
        let mut record = [0; WORDS];
        for (place, word) in record.iter_mut().enumerate() {
            *word = number * 10 + place as u64;
        }
        record
    }

    #[test]
    fn records_come_out_in_order_and_whole() {
        let (mut writer, mut reader) = ring(8);
        assert_eq!(reader.pop(), None);
        for number in 0..5 {
            assert!(writer.push(&record(number)));
        }
        assert_eq!(reader.waiting(), 5);
        for number in 0..5 {
            assert_eq!(reader.pop(), Some(record(number)));
        }
        assert_eq!(reader.pop(), None);
    }

    #[test]
    fn a_full_ring_drops_and_counts() {
        let (mut writer, mut reader) = ring(4);
        assert_eq!(reader.capacity(), 4);
        for number in 0..4 {
            assert!(writer.push(&record(number)));
        }
        assert!(!writer.push(&record(4)));
        assert!(!writer.push(&record(5)));
        assert_eq!(reader.dropped(), 2);
        // What was in is still whole, and room comes back.
        assert_eq!(reader.pop(), Some(record(0)));
        assert!(writer.push(&record(6)));
        for number in [1, 2, 3, 6] {
            assert_eq!(reader.pop(), Some(record(number)));
        }
    }

    #[test]
    fn the_size_is_rounded_up() {
        let (_, reader) = ring(5);
        assert_eq!(reader.capacity(), 8);
        let (_, reader) = ring(0);
        assert_eq!(reader.capacity(), 2);
    }

    #[test]
    fn two_threads_lose_nothing() {
        const RECORDS: u64 = 200_000;
        let (mut writer, mut reader) = ring(64);
        let producer = std::thread::spawn(move || {
            let mut number = 0;
            while number < RECORDS {
                if writer.push(&record(number)) {
                    number += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let mut expected = 0;
        while expected < RECORDS {
            match reader.pop() {
                Some(found) => {
                    assert_eq!(found, record(expected));
                    expected += 1;
                }
                None => std::thread::yield_now(),
            }
        }
        producer.join().expect("the writer ended badly");
        assert_eq!(reader.pop(), None);
    }
}
