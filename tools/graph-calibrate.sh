#!/bin/sh
# graph-calibrate.sh — what the reference load of requirement R-04 is
# made of on this machine, and what every kind of node costs (step
# P1.7d, ADR 0011).
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/graph-calibrate.sh
#
# A load is a cascade of filter stages and of trims (see dawos_rt::dsp
# in the sources of the engine). How many of each make 10, 40 and 150 ns
# per frame depends on three figures: what a load costs per frame before
# any filtering, what a stage adds and what a trim adds. This measures
# them, with the engine itself, on one live CPU, without a sound card:
#
#   1. the graph calibrate.graph at 96 kHz with 192 frames, the quantum
#      of the reference session: three cascades of a known size give the
#      three figures (the line "load_costs:");
#   2. the same graph again with those figures given to the engine
#      ("--load-costs"), at 48, 96 and 192 kHz: what the three classes
#      then cost, which should be 10, 40 and 150 ns per frame at 96 kHz;
#   3. the kernel of dawos-dspbench on the same CPU: one vector of its
#      four instances should cost what a stage costs here;
#   4. a load fed with numbers too small for the normal form of a float,
#      with the live threads taking them for zero (as always) and
#      leaving them as they are: what FTZ and DAZ save.
#
# The figures go to load-costs.txt in the directory of the results;
# tools/graph-threads.sh takes the newest with LOAD_COSTS=auto. Once
# they are written in the sources (Costs::default in dawos-graph), the
# engine uses them by itself.
#
# Results go to /var/lib/dawos/measure/calibrate-<time>[-LABEL].

set -u

label=${1:-}
ENGINE=${DAWOS_ENGINE:-dawos-engine}
DSPBENCH=${DAWOS_DSPBENCH:-/usr/libexec/dawos/dspbench}
GRAPHS=${DAWOS_GRAPHS_DIR:-/usr/share/dawos/graphs}
BASE=/var/lib/dawos/measure
SECONDS_EACH=${SECONDS_EACH:-20}
# Options of the engine for every run; "--no-rt" to try the script where
# no real-time priority can be had (a virtual machine).
ENGINE_OPTIONS=${ENGINE_OPTIONS:---require-rt}

die() {
	echo "graph-calibrate: $*" >&2
	exit 1
}

field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

# Average of node $2 in the table of the nodes one by one, in
# microseconds.
node_us() {
	awk -v node="$2" '
		/^node +kind +thread/ { inside = 1; next }
		inside && NF == 0 { inside = 0 }
		inside && $1 == node { print $6 }' "$1" | head -n 1
}

# The same in nanoseconds per frame: $3 frames in the quantum.
node_ns_per_frame() {
	awk -v us="$(node_us "$1" "$2")" -v frames="$3" 'BEGIN {
		if (us == "") print "-"; else printf "%.3f", us * 1000 / frames }'
}

[ "$(id -u)" = 0 ] || die "must run as root"
[ -r "${GRAPHS}/calibrate.graph" ] || die "no graph ${GRAPHS}/calibrate.graph"

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/calibrate-${stamp}${label:+-${label}}"
mkdir -p "${dir}" || die "cannot create ${dir}"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "engine: $("${ENGINE}" --version 2> /dev/null)"
	echo "seconds: ${SECONDS_EACH}"
} > "${dir}/meta.txt"
echo "directory: ${dir}"

# One run of the engine: $1 name of the output, $2 graph, $3 rate, the
# rest are more options.
run() {
	name=$1
	graph=$2
	rate=$3
	shift 3
	# shellcheck disable=SC2086  # ENGINE_OPTIONS is several words
	"${ENGINE}" run --graph "${GRAPHS}/${graph}" --rate "${rate}" --threads 1 --graph-nodes \
		--seconds "${SECONDS_EACH}" ${ENGINE_OPTIONS} "$@" \
		> "${dir}/${name}.txt" 2> "${dir}/${name}.err"
}

{
	# ---- 1. The three figures, at the quantum of the reference session.
	run measure calibrate.graph 96000
	frames=$(field "${dir}/measure.txt" quantum_frames)
	[ -n "${frames}" ] || die "the engine did not run: $(tail -n 1 "${dir}/measure.err")"
	costs=$(awk -v frame="$(node_us "${dir}/measure.txt" frame)" \
		-v stages="$(node_us "${dir}/measure.txt" stages)" \
		-v trims="$(node_us "${dir}/measure.txt" trims)" -v frames="${frames}" 'BEGIN {
			if (frame == "" || stages == "" || trims == "") exit 1
			printf "%.3f,%.3f,%.3f", frame * 1000 / frames,
				(stages - frame) * 1000 / 64 / frames, (trims - frame) * 1000 / 64 / frames }') ||
		die "the cascades of calibrate.graph are not in the report"
	echo "${costs}" > "${dir}/load-costs.txt"
	echo "at 96 kHz with ${frames} frames, nanoseconds per frame:"
	echo "load_costs: ${costs} (a load before any filtering, a stage, a trim)"
	echo "a_stage_in_a_cascade_of_4: $(awk -v a="$(node_us "${dir}/measure.txt" stages4)" \
		-v b="$(node_us "${dir}/measure.txt" frame)" -v frames="${frames}" \
		'BEGIN { printf "%.3f", (a - b) * 1000 / 4 / frames }')"
	echo "a_stage_in_a_cascade_of_256: $(awk -v a="$(node_us "${dir}/measure.txt" stages256)" \
		-v b="$(node_us "${dir}/measure.txt" frame)" -v frames="${frames}" \
		'BEGIN { printf "%.3f", (a - b) * 1000 / 256 / frames }')"
	echo "the_classes_with_the_figures_of_the_sources: light $(node_ns_per_frame "${dir}/measure.txt" light "${frames}"), medium $(node_ns_per_frame "${dir}/measure.txt" medium "${frames}"), heavy $(node_ns_per_frame "${dir}/measure.txt" heavy "${frames}")"
	grep '^graph_load_of_' "${dir}/measure.txt" | sed 's/^/  in the sources: /'

	# ---- 2. The classes with the figures just measured.
	echo
	echo "the classes with the figures just measured, nanoseconds per frame:"
	printf '%-8s %-8s %10s %10s %10s  %s\n' "rate" "frames" "light" "medium" "heavy" "made of"
	for rate in 48000 96000 192000; do
		run "classes-${rate}" calibrate.graph "${rate}" --load-costs "${costs}"
		out="${dir}/classes-${rate}.txt"
		frames=$(field "${out}" quantum_frames)
		printf '%-8s %-8s %10s %10s %10s  %s\n' "${rate}" "${frames}" \
			"$(node_ns_per_frame "${out}" light "${frames}")" \
			"$(node_ns_per_frame "${out}" medium "${frames}")" \
			"$(node_ns_per_frame "${out}" heavy "${frames}")" \
			"$(grep '^graph_load_of_' "${out}" | sed 's/^graph_load_of_\([0-9]*\)_ns: \([0-9]*\) stages and \([0-9]*\) trims.*/\1: \2+\3/' | tr '\n' ' ')"
	done

	# ---- 3. The kernel of dawos-dspbench on the first live CPU.
	echo
	live=$(field "${dir}/measure.txt" live_cpus | sed 's/[^0-9].*//')
	if [ -x "${DSPBENCH}" ] && [ -n "${live}" ]; then
		taskset -c "${live}" "${DSPBENCH}" --block 192 --stages 64 --streams 1 --seconds 2 \
			> "${dir}/dspbench.txt" 2>&1
		instance=$(sed -n 's/.*ns_per_frame_stage_median=\([0-9.]*\).*/\1/p' "${dir}/dspbench.txt")
		echo "dspbench_on_cpu_${live}: ${instance:-?} ns per frame and per stage of one instance; a vector of four: $(awk -v ns="${instance:-0}" 'BEGIN { printf "%.3f", ns * 4 }') (a stage here: $(echo "${costs}" | cut -d, -f2))"
	else
		echo "dspbench: not run (no ${DSPBENCH}, or the live CPUs are not known)"
	fi

	# ---- 4. Numbers too small for the normal form.
	echo
	run denormals-flushed denormals.graph 96000 --load-costs "${costs}"
	run denormals-kept denormals.graph 96000 --load-costs "${costs}" --keep-denormals
	frames=$(field "${dir}/denormals-flushed.txt" quantum_frames)
	echo "a light load fed with numbers too small for the normal form, nanoseconds per frame:"
	echo "denormals_taken_for_zero: $(node_ns_per_frame "${dir}/denormals-flushed.txt" filter "${frames}")"
	echo "denormals_kept: $(node_ns_per_frame "${dir}/denormals-kept.txt" filter "${frames}")"

	# ---- The other kinds, for the costs the compiler starts from.
	echo
	echo "the other kinds, microseconds for a quantum of 96, 192 and 384 frames:"
	printf '%-10s %-10s %10s %10s %10s\n' "node" "kind" "96" "192" "384"
	for node in source tone click softer later sum2 sum8 bus2 bus8 seen wait; do
		printf '%-10s %-10s %10s %10s %10s\n' "${node}" \
			"$(awk -v node="${node}" '$1 == node && $3 == "0" { print $2 }' "${dir}/classes-96000.txt" | head -n 1)" \
			"$(node_us "${dir}/classes-48000.txt" "${node}")" \
			"$(node_us "${dir}/classes-96000.txt" "${node}")" \
			"$(node_us "${dir}/classes-192000.txt" "${node}")"
	done
	echo
	echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} | tee "${dir}/summary.txt"
