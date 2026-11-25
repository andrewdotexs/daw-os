//! Vector code: the filter cascade of the reference load (ADR 0011).
//!
//! The "synthetic plug-in" of requirement R-04 is real filtering whose
//! amount is calibrated to a declared cost. Its kernel is the one
//! `dawos-dspbench` times (experiment E-06): second-order sections in
//! the transposed direct form II, one 256-bit vector of eight values
//! per frame and per stage, two fused multiply-adds one after the other
//! from one frame to the next. Here the eight values are one stereo
//! instance split into four bands, left and right of each side by side:
//! every band goes through its own chain of all-pass sections, and the
//! four are averaged at the end. An all-pass section changes the phase
//! and not the level: what comes out is as loud as what went in.
//!
//! What a pass costs:
//!
//! - a **stage**: every frame waits for the frame before, through two
//!   multiply-adds; its cost is the time those take, whatever else the
//!   processor could do meanwhile;
//! - a **trim**: a plain multiplication of every frame, some tenths of
//!   what a stage costs: it brings the cost of an instance to the
//!   declared one more finely than whole stages can;
//! - the **spread and the collection**: from two channels to eight
//!   values and back, once per frame.
//!
//! The functions with the vector instructions are compiled for AVX2 and
//! FMA whatever the rest of the program is compiled for, and are called
//! only where the processor has both (the one `unsafe` call is in
//! `sys`); elsewhere the same arithmetic is done a value at a time. The
//! two give the same samples to the last bit: a fused multiply-add is
//! one operation with one rounding in both.
//!
//! Nothing here allocates after [`Cascade::new`].

use crate::sys;

/// Values in a 256-bit vector of 32-bit floats.
pub const LANES: usize = 8;

/// One frame of a stereo instance split into four bands: left and right
/// of the first band, then of the second, and so on.
pub type Frame = [f32; LANES];

/// One second-order section, with what it keeps from frame to frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stage {
    b0: Frame,
    b1: Frame,
    b2: Frame,
    a1: Frame,
    a2: Frame,
    z1: Frame,
    z2: Frame,
}

/// A cascade of sections and trims for one stereo instance.
#[derive(Clone, Debug)]
pub struct Cascade {
    stages: Vec<Stage>,
    trims: u32,
    /// What a trim multiplies by: read from here at every pass, so that
    /// the compiler cannot know it is one.
    trim: f32,
}

/// Whether the processor has the vector instructions the cascade is
/// written for (AVX2 and FMA). Asked of the processor once; it does not
/// enter the kernel.
#[must_use]
pub fn vector_unit() -> bool {
    sys::has_avx2_and_fma()
}

impl Cascade {
    /// A cascade of `stages` all-pass sections and `trims` trims. The
    /// sections are tuned apart, stage by stage and band by band, from
    /// one hundredth of the sample rate to a fifth.
    #[must_use]
    pub fn new(stages: u32, trims: u32) -> Self {
        let sections = f64::from(stages) * (LANES / 2) as f64;
        let stages = (0..stages)
            .map(|stage| {
                let mut built = Stage {
                    b0: [0.0; LANES],
                    b1: [0.0; LANES],
                    b2: [1.0; LANES],
                    a1: [0.0; LANES],
                    a2: [0.0; LANES],
                    z1: [0.0; LANES],
                    z2: [0.0; LANES],
                };
                for lane in 0..LANES {
                    let band = (lane / 2) as f64;
                    let place =
                        (f64::from(stage) * (LANES / 2) as f64 + band + 1.0) / (sections + 1.0);
                    let angle = std::f64::consts::PI * (0.02 + 0.4 * place);
                    // The usual formulas of an all-pass section, with a
                    // quality of one over the square root of two.
                    let alpha = angle.sin() / std::f64::consts::SQRT_2;
                    let b0 = (1.0 - alpha) / (1.0 + alpha);
                    let b1 = -2.0 * angle.cos() / (1.0 + alpha);
                    built.b0[lane] = b0 as f32;
                    built.b1[lane] = b1 as f32;
                    built.a1[lane] = b1 as f32;
                    built.a2[lane] = b0 as f32;
                }
                built
            })
            .collect();
        Self {
            stages,
            trims,
            trim: 1.0,
        }
    }

    /// Sections in the cascade.
    #[must_use]
    pub fn stages(&self) -> usize {
        self.stages.len()
    }

    /// One block of a stereo instance: from `left` and `right` to
    /// `out_left` and `out_right`, through `work`, which holds a frame
    /// for every frame of the block. The shortest of the five decides
    /// how many frames are done. Compute phase: no system call, no
    /// allocation.
    #[inline]
    pub fn run(
        &mut self,
        left: &[f32],
        right: &[f32],
        out_left: &mut [f32],
        out_right: &mut [f32],
        work: &mut [Frame],
    ) {
        let block = Block {
            left,
            right,
            out_left,
            out_right,
            work,
            trims: self.trims,
            trim: self.trim,
        };
        sys::cascade(&mut self.stages, block);
    }
}

/// What one run of a cascade works on.
pub(crate) struct Block<'a> {
    left: &'a [f32],
    right: &'a [f32],
    out_left: &'a mut [f32],
    out_right: &'a mut [f32],
    work: &'a mut [Frame],
    trims: u32,
    trim: f32,
}

impl Block<'_> {
    /// Frames in the block: the shortest of its five slices.
    #[inline(always)]
    fn frames(&self) -> usize {
        self.left
            .len()
            .min(self.right.len())
            .min(self.out_left.len())
            .min(self.out_right.len())
            .min(self.work.len())
    }
}

#[cfg(target_arch = "x86_64")]
mod vector {
    use super::{Block, Frame, Stage};
    use std::arch::x86_64::{
        __m256, _mm256_castps256_ps128, _mm256_extractf128_ps, _mm256_fmadd_ps, _mm256_fnmadd_ps,
        _mm256_mul_ps, _mm256_set1_ps, _mm256_setr_ps, _mm_cvtss_f32, _mm_permute_ps,
    };

    // Written value by value, and compiled to one move of the whole
    // vector: the intrinsics that take a pointer would need `unsafe`.
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    fn load(frame: Frame) -> __m256 {
        _mm256_setr_ps(
            frame[0], frame[1], frame[2], frame[3], frame[4], frame[5], frame[6], frame[7],
        )
    }

    #[inline]
    #[target_feature(enable = "avx2,fma")]
    fn store(vector: __m256) -> Frame {
        let low = _mm256_castps256_ps128(vector);
        let high = _mm256_extractf128_ps::<1>(vector);
        [
            _mm_cvtss_f32(low),
            _mm_cvtss_f32(_mm_permute_ps::<1>(low)),
            _mm_cvtss_f32(_mm_permute_ps::<2>(low)),
            _mm_cvtss_f32(_mm_permute_ps::<3>(low)),
            _mm_cvtss_f32(high),
            _mm_cvtss_f32(_mm_permute_ps::<1>(high)),
            _mm_cvtss_f32(_mm_permute_ps::<2>(high)),
            _mm_cvtss_f32(_mm_permute_ps::<3>(high)),
        ]
    }

    /// The cascade with a vector per frame. The state of a stage stays
    /// in two registers for the whole block.
    #[target_feature(enable = "avx2,fma")]
    pub(crate) fn run(stages: &mut [Stage], block: Block<'_>) {
        let frames = block.frames();
        let work = &mut block.work[..frames];
        super::spread(block.left, block.right, work);
        for stage in stages.iter_mut() {
            let b0 = load(stage.b0);
            let b1 = load(stage.b1);
            let b2 = load(stage.b2);
            let a1 = load(stage.a1);
            let a2 = load(stage.a2);
            let mut z1 = load(stage.z1);
            let mut z2 = load(stage.z2);
            for frame in work.iter_mut() {
                let x = load(*frame);
                let y = _mm256_fmadd_ps(b0, x, z1);
                z1 = _mm256_fnmadd_ps(a1, y, _mm256_fmadd_ps(b1, x, z2));
                z2 = _mm256_fnmadd_ps(a2, y, _mm256_mul_ps(b2, x));
                *frame = store(y);
            }
            stage.z1 = store(z1);
            stage.z2 = store(z2);
        }
        for _ in 0..block.trims {
            let by = _mm256_set1_ps(std::hint::black_box(block.trim));
            for frame in work.iter_mut() {
                *frame = store(_mm256_mul_ps(load(*frame), by));
            }
        }
        super::collect(work, block.out_left, block.out_right);
    }
}

#[cfg(target_arch = "x86_64")]
pub(crate) use vector::run as run_vector;

/// The cascade a value at a time: the same operations in the same
/// order, for a processor without the vector instructions.
pub(crate) fn run_plain(stages: &mut [Stage], block: Block<'_>) {
    let frames = block.frames();
    let work = &mut block.work[..frames];
    spread(block.left, block.right, work);
    for stage in stages.iter_mut() {
        for frame in work.iter_mut() {
            for (lane, value) in frame.iter_mut().enumerate() {
                let x = *value;
                let y = stage.b0[lane].mul_add(x, stage.z1[lane]);
                stage.z1[lane] =
                    (-stage.a1[lane]).mul_add(y, stage.b1[lane].mul_add(x, stage.z2[lane]));
                stage.z2[lane] = (-stage.a2[lane]).mul_add(y, stage.b2[lane] * x);
                *value = y;
            }
        }
    }
    for _ in 0..block.trims {
        let by = std::hint::black_box(block.trim);
        for frame in work.iter_mut() {
            for value in frame.iter_mut() {
                *value *= by;
            }
        }
    }
    collect(work, block.out_left, block.out_right);
}

/// From two channels to the eight values of every frame.
#[inline(always)]
fn spread(left: &[f32], right: &[f32], work: &mut [Frame]) {
    for ((frame, &left), &right) in work.iter_mut().zip(left).zip(right) {
        *frame = [left, right, left, right, left, right, left, right];
    }
}

/// The four bands averaged, left and right.
#[inline(always)]
fn collect(work: &[Frame], out_left: &mut [f32], out_right: &mut [f32]) {
    for ((frame, left), right) in work.iter().zip(out_left).zip(out_right) {
        *left = (frame[0] + frame[2] + frame[4] + frame[6]) * 0.25;
        *right = (frame[1] + frame[3] + frame[5] + frame[7]) * 0.25;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES: usize = 96;

    fn noise(seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..FRAMES * 40)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / 8_388_608.0 - 1.0
            })
            .collect()
    }

    /// Run `cascade` over `left` and `right` block by block, with or
    /// without the vector instructions.
    fn through(
        cascade: &mut Cascade,
        left: &[f32],
        right: &[f32],
        vector: bool,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut out = (vec![0.0; left.len()], vec![0.0; right.len()]);
        let mut work = vec![[0.0; LANES]; FRAMES];
        for block in 0..left.len() / FRAMES {
            let range = block * FRAMES..(block + 1) * FRAMES;
            let block = Block {
                left: &left[range.clone()],
                right: &right[range.clone()],
                out_left: &mut out.0[range.clone()],
                out_right: &mut out.1[range],
                work: &mut work,
                trims: cascade.trims,
                trim: cascade.trim,
            };
            if vector {
                sys::cascade(&mut cascade.stages, block);
            } else {
                run_plain(&mut cascade.stages, block);
            }
        }
        out
    }

    fn power(samples: &[f32]) -> f64 {
        samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / samples.len() as f64
    }

    #[test]
    fn the_vector_code_and_the_plain_code_give_the_same_samples() {
        if !vector_unit() {
            return;
        }
        let (left, right) = (noise(1), noise(2));
        let with = through(&mut Cascade::new(7, 3), &left, &right, true);
        let without = through(&mut Cascade::new(7, 3), &left, &right, false);
        assert_eq!(with, without);
    }

    #[test]
    fn nothing_but_trims_gives_back_the_input() {
        let (left, right) = (noise(3), noise(4));
        let out = through(&mut Cascade::new(0, 5), &left, &right, true);
        assert_eq!(out.0, left);
        assert_eq!(out.1, right);
    }

    /// One all-pass section keeps the power of the signal. (A cascade
    /// averages four bands with different phases, and loses some.)
    #[test]
    fn the_cascade_is_stable_and_no_louder_than_its_input() {
        let (left, right) = (noise(5), noise(6));
        let before = power(&left);
        for stages in [1, 6, 24, 90] {
            let out = through(&mut Cascade::new(stages, 0), &left, &right, true);
            let after = power(&out.0[FRAMES * 20..]);
            assert!(out.0.iter().all(|sample| sample.is_finite()));
            assert!(
                after < before * 1.01,
                "{stages} stages: {after} from {before}"
            );
            assert!(
                after > before * 0.05,
                "{stages} stages: {after} from {before}"
            );
            // Left and right do not mix.
            assert_ne!(out.0, out.1);
        }
    }

    #[test]
    fn silence_in_silence_out_and_the_state_dies_away() {
        let silence = vec![0.0; FRAMES * 40];
        let mut cascade = Cascade::new(6, 1);
        let (left, right) = (noise(7), noise(8));
        let _ = through(&mut cascade, &left, &right, true);
        let out = through(&mut cascade, &silence, &silence, true);
        // The tail of what was there, then nothing.
        assert!(out.0[..FRAMES].iter().any(|sample| *sample != 0.0));
        assert!(out.0[FRAMES * 30..]
            .iter()
            .all(|sample| sample.abs() < 1e-6));
    }

    #[test]
    fn the_shortest_slice_decides_the_block() {
        let mut cascade = Cascade::new(2, 0);
        let mut out = ([9.0_f32; 8], [9.0_f32; 8]);
        let mut work = [[0.0; LANES]; 4];
        cascade.run(&[1.0; 8], &[1.0; 6], &mut out.0, &mut out.1, &mut work);
        assert!(out.0[..4].iter().all(|sample| *sample != 9.0));
        assert!(out.0[4..].iter().all(|sample| *sample == 9.0));
    }
}
