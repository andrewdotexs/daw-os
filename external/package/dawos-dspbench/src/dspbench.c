/*
 * dspbench — what a cascade of second-order filters costs on the CPU it
 * runs on, per stereo frame (experiment E-06, ADR 0011).
 *
 * The reference load of dawOS (the "synthetic plug-in" of requirement
 * R-04) is a cascade of second-order filter stages whose number is
 * calibrated to a declared cost. This program is that kernel and
 * nothing else: it times it, so that the same work can be compared on a
 * performance core and on an efficiency core.
 *
 * The vector unit is used at its full AVX2 width: one 256-bit vector
 * holds eight values, that is four stereo instances side by side. The
 * cost is reported per frame (a left and a right sample) of one
 * instance, and per filter stage.
 *
 * A filter with feedback is a chain: each output waits for the previous
 * one. With one stream the processor can do little in parallel and the
 * time is set by how long one multiply-add takes; with four independent
 * streams advanced together the chains overlap and the time is set by
 * how many multiply-adds the core can run at once. Real plug-ins sit
 * between the two, so both are measured (--streams 1 or 4).
 *
 * Where the buffers lie matters. A processor decides whether a read
 * must wait for a write still in flight by comparing the low 12 bits of
 * the two addresses only: buffers that start at the same offset within
 * a page make each stream wait for its neighbour, which is an accident
 * of layout and not a property of the core. Every buffer here starts at
 * an offset of its own (see place()); "--layout same" puts them all at
 * the same offset instead, to see what that accident costs.
 *
 * Usage: dspbench [--block FRAMES] [--stages N] [--streams 1|4]
 *                 [--seconds S] [--layout apart|same]
 * It runs on whatever CPU it is started on: bind it with taskset.
 */

#define _GNU_SOURCE
#include <immintrin.h>
#include <math.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define LANES 8			/* values in one 256-bit vector */
#define INSTANCES (LANES / 2)	/* stereo instances side by side */
#define MAX_STAGES 64
#define MAX_STREAMS 4
#define MAX_BATCHES 100000

struct stage {
	float b0, b1, b2, a1, a2;
	__m256 z1[MAX_STREAMS], z2[MAX_STREAMS];
};

static uint64_t now_ns(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

/* Low-pass sections with different corner frequencies (the usual
 * "cookbook" formulas), so that the stages are not all the same. */
static void design(struct stage *st, int stages)
{
	for (int s = 0; s < stages; s++) {
		double w = 2.0 * M_PI * (0.02 + 0.4 * (s + 1) / (stages + 1.0)) / 2.0;
		double alpha = sin(w) / (2.0 * 0.7071);
		double a0 = 1.0 + alpha;

		st[s].b0 = (float)((1.0 - cos(w)) / 2.0 / a0);
		st[s].b1 = (float)((1.0 - cos(w)) / a0);
		st[s].b2 = st[s].b0;
		st[s].a1 = (float)(-2.0 * cos(w) / a0);
		st[s].a2 = (float)((1.0 - alpha) / a0);
		for (int k = 0; k < MAX_STREAMS; k++)
			st[s].z1[k] = st[s].z2[k] = _mm256_setzero_ps();
	}
}

/*
 * One frame of one stream through one stage: output from input and
 * state, then the two state values for the next frame.
 */
#define STEP(z1, z2, src, dst, at) do {					\
		const __m256 x = _mm256_load_ps((src) + (at));		\
		const __m256 y = _mm256_fmadd_ps(b0, x, z1);		\
									\
		z1 = _mm256_fnmadd_ps(a1, y, _mm256_fmadd_ps(b1, x, z2)); \
		z2 = _mm256_fnmadd_ps(a2, y, _mm256_mul_ps(b2, x));	\
		_mm256_store_ps((dst) + (at), y);			\
	} while (0)

#define COEFFICIENTS(st)						\
	const __m256 b0 = _mm256_set1_ps((st).b0);			\
	const __m256 b1 = _mm256_set1_ps((st).b1);			\
	const __m256 b2 = _mm256_set1_ps((st).b2);			\
	const __m256 a1 = _mm256_set1_ps((st).a1);			\
	const __m256 a2 = _mm256_set1_ps((st).a2)

/*
 * One pass of the whole cascade over a block of one stream. The first
 * stage reads the input, the others work on the output in place.
 */
static __attribute__((noinline)) void
cascade_1(struct stage *st, int stages, float *const *in, float *const *out, int frames)
{
	float *const dst = out[0];

	for (int s = 0; s < stages; s++) {
		COEFFICIENTS(st[s]);
		const float *src = s ? dst : in[0];
		__m256 p1 = st[s].z1[0], p2 = st[s].z2[0];

		for (int i = 0; i < frames; i++)
			STEP(p1, p2, src, dst, (size_t)i * LANES);
		st[s].z1[0] = p1;
		st[s].z2[0] = p2;
	}
}

/*
 * The same for four independent streams advanced together, frame by
 * frame. Written out stream by stream so that the state of each stays
 * in a register of its own: an array would send it through memory and
 * lengthen every chain.
 */
static __attribute__((noinline)) void
cascade_4(struct stage *st, int stages, float *const *in, float *const *out, int frames)
{
	float *const d0 = out[0], *const d1 = out[1];
	float *const d2 = out[2], *const d3 = out[3];

	for (int s = 0; s < stages; s++) {
		COEFFICIENTS(st[s]);
		const float *s0 = s ? d0 : in[0], *s1 = s ? d1 : in[1];
		const float *s2 = s ? d2 : in[2], *s3 = s ? d3 : in[3];
		__m256 p1 = st[s].z1[0], p2 = st[s].z2[0];
		__m256 q1 = st[s].z1[1], q2 = st[s].z2[1];
		__m256 r1 = st[s].z1[2], r2 = st[s].z2[2];
		__m256 t1 = st[s].z1[3], t2 = st[s].z2[3];

		for (int i = 0; i < frames; i++) {
			const size_t at = (size_t)i * LANES;

			STEP(p1, p2, s0, d0, at);
			STEP(q1, q2, s1, d1, at);
			STEP(r1, r2, s2, d2, at);
			STEP(t1, t2, s3, d3, at);
		}
		st[s].z1[0] = p1, st[s].z2[0] = p2;
		st[s].z1[1] = q1, st[s].z2[1] = q2;
		st[s].z1[2] = r1, st[s].z2[2] = r2;
		st[s].z1[3] = t1, st[s].z2[3] = t2;
	}
}

/*
 * Buffer number "slot" of an arena, "bytes" long. Slots never overlap,
 * and each starts 512 bytes further into its page than the one before:
 * eight buffers (four inputs, four outputs) cover the eight offsets a
 * page has room for, so that no stream reads at the offset another one
 * is writing at.
 */
#define PAGE 4096
#define SLOT_STEP 512

static float *place(char *arena, size_t bytes, int slot, int apart)
{
	const size_t pitch = (bytes + PAGE - 1) / PAGE * PAGE + PAGE;

	return (float *)(arena + (size_t)slot * pitch +
			 (apart ? (size_t)slot * SLOT_STEP : 0));
}

static int by_value(const void *a, const void *b)
{
	const double x = *(const double *)a, y = *(const double *)b;

	return (x > y) - (x < y);
}

int main(int argc, char **argv)
{
	int frames = 256, stages = 8, streams = 1, apart = 1;
	double seconds = 2.0;
	static struct stage st[MAX_STAGES];
	static double batch_ns[MAX_BATCHES];

	for (int i = 1; i < argc; i++) {
		const char *value = i + 1 < argc ? argv[i + 1] : "";

		if (!strcmp(argv[i], "--block"))
			frames = atoi(value), i++;
		else if (!strcmp(argv[i], "--stages"))
			stages = atoi(value), i++;
		else if (!strcmp(argv[i], "--streams"))
			streams = atoi(value), i++;
		else if (!strcmp(argv[i], "--seconds"))
			seconds = atof(value), i++;
		else if (!strcmp(argv[i], "--layout") && !strcmp(value, "apart"))
			apart = 1, i++;
		else if (!strcmp(argv[i], "--layout") && !strcmp(value, "same"))
			apart = 0, i++;
		else {
			fprintf(stderr, "usage: %s [--block FRAMES] [--stages N] [--streams 1|4] [--seconds S] [--layout apart|same]\n", argv[0]);
			return 2;
		}
	}
	if (frames < 16 || frames > 65536 || stages < 1 || stages > MAX_STAGES ||
	    (streams != 1 && streams != 4) || seconds < 0.1 || seconds > 600) {
		fprintf(stderr, "dspbench: block 16-65536, stages 1-%d, streams 1 or 4, seconds 0.1-600\n", MAX_STAGES);
		return 2;
	}
	if (!__builtin_cpu_supports("avx2") || !__builtin_cpu_supports("fma")) {
		fprintf(stderr, "dspbench: this processor has no AVX2 with FMA\n");
		return 3;
	}

	/* Very small numbers would otherwise be handled in microcode and
	 * slow the filters down by an order of magnitude: treat them as
	 * zero, as every audio engine does. */
	_mm_setcsr(_mm_getcsr() | 0x8040);

	/* Outputs in the even slots, inputs in the odd ones. */
	const size_t values = (size_t)frames * LANES;
	const size_t bytes = values * sizeof(float);
	const size_t pitch = (bytes + PAGE - 1) / PAGE * PAGE + PAGE;
	char *arena = aligned_alloc(PAGE, 2 * MAX_STREAMS * (pitch + SLOT_STEP));
	float *in[MAX_STREAMS], *out[MAX_STREAMS];

	if (!arena) {
		fprintf(stderr, "dspbench: out of memory\n");
		return 1;
	}
	/* White noise between -1 and 1, the same on every run. */
	uint32_t seed = 0x9e3779b9u;
	for (int k = 0; k < streams; k++) {
		out[k] = place(arena, bytes, 2 * k, apart);
		in[k] = place(arena, bytes, 2 * k + 1, apart);
		for (size_t i = 0; i < values; i++) {
			seed ^= seed << 13;
			seed ^= seed >> 17;
			seed ^= seed << 5;
			in[k][i] = (float)((double)seed / 2147483648.0 - 1.0);
		}
		memset(out[k], 0, bytes);
	}
	design(st, stages);

	void (*pass)(struct stage *, int, float *const *, float *const *, int) =
		streams == 4 ? cascade_4 : cascade_1;
	const int cpu_at_start = sched_getcpu();

	/* Warm up, and find how many passes make a batch of about 1 ms. */
	uint64_t t0 = now_ns();
	int warm = 0;
	while (now_ns() - t0 < 100000000ull) {
		pass(st, stages, in, out, frames);
		warm++;
	}
	int per_batch = (int)((double)warm / 100.0);
	if (per_batch < 1)
		per_batch = 1;

	/* One frame of one instance through one stage: the unit of cost. */
	const double work_per_pass = (double)frames * INSTANCES * streams * stages;
	const uint64_t deadline = now_ns() + (uint64_t)(seconds * 1e9);
	int batches = 0;

	while (batches < MAX_BATCHES && now_ns() < deadline) {
		const uint64_t start = now_ns();

		for (int r = 0; r < per_batch; r++)
			pass(st, stages, in, out, frames);
		batch_ns[batches++] = (double)(now_ns() - start) / (work_per_pass * per_batch);
	}
	qsort(batch_ns, (size_t)batches, sizeof(double), by_value);

	double sum = 0.0;
	for (int k = 0; k < streams; k++)
		for (size_t i = 0; i < values; i++)
			sum += out[k][i];

	printf("cpu=%d cpu_at_end=%d block=%d stages=%d streams=%d layout=%s batches=%d "
	       "ns_per_frame_stage_min=%.4f ns_per_frame_stage_median=%.4f "
	       "ns_per_frame_cascade_median=%.3f checksum=%.6f\n",
	       cpu_at_start, sched_getcpu(), frames, stages, streams,
	       apart ? "apart" : "same", batches,
	       batch_ns[0], batch_ns[batches / 2], batch_ns[batches / 2] * stages, sum);
	return 0;
}
