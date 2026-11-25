#!/bin/sh
# graph-threads.sh — one graph on one, two, three and four live threads:
# what the compiler expected of each cycle and what the threads took
# (step P1.7c).
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/graph-threads.sh
#
# For each number of THREADS the engine runs the graph GRAPH at RATE for
# SECONDS seconds, without a sound card: a timer gives the periods, the
# inputs of the graph are silence and its outputs go nowhere. What is
# measured is the cycle: how long the threads compute, how long they
# wait for each other, whether every cycle gets to the end of the graph.
#
# The compiler says how long a cycle should last on so many threads
# (expected_us: the makespan of its schedule, made with the costs the
# nodes declare). The engine says how long the compute phase lasted on
# the thread where it was longest (slowest_avg and slowest_max), and
# speed_up is that time on the first number of threads over that time on
# this one. critical is the longest chain of nodes that wait for each
# other, as the sum of what its nodes took on average: no number of
# threads makes a cycle shorter.
#
# A load of the graph is made of what the sources of the engine say
# (Costs::default in dawos-graph) unless LOAD_COSTS says otherwise:
# three figures FRAME,STAGE,TRIM as tools/graph-calibrate.sh prints
# them, or "auto" for those of its newest run on this machine.
#
# Results go to /var/lib/dawos/measure/graph-<time>[-LABEL].

set -u

label=${1:-}
ENGINE=${DAWOS_ENGINE:-dawos-engine}
BASE=/var/lib/dawos/measure
GRAPH=${GRAPH:-/usr/share/dawos/graphs/tracks.graph}
RATE=${RATE:-96000}
THREADS=${THREADS:-1 2 3 4}
SECONDS_EACH=${SECONDS_EACH:-30}
# Options of the engine for every run; "--no-rt" to try the script where
# no real-time priority can be had (a virtual machine).
ENGINE_OPTIONS=${ENGINE_OPTIONS:---require-rt}
LOAD_COSTS=${LOAD_COSTS:-}

die() {
	echo "graph-threads: $*" >&2
	exit 1
}

field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

[ "$(id -u)" = 0 ] || die "must run as root"
[ -r "${GRAPH}" ] || die "no graph ${GRAPH}"
costs_from="the sources of the engine"
if [ "${LOAD_COSTS}" = auto ]; then
	newest=$(find "${BASE}" -maxdepth 2 -name load-costs.txt -path '*/calibrate-*' | sort | tail -n 1)
	[ -n "${newest}" ] || die "LOAD_COSTS=auto: no calibration in ${BASE}: run tools/graph-calibrate.sh first"
	LOAD_COSTS=$(cat "${newest}")
	costs_from=${newest}
elif [ -n "${LOAD_COSTS}" ]; then
	costs_from="LOAD_COSTS"
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/graph-${stamp}${label:+-${label}}"
mkdir -p "${dir}" || die "cannot create ${dir}"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "engine: $("${ENGINE}" --version 2> /dev/null)"
	echo "graph: ${GRAPH}"
	echo "rate: ${RATE}"
	echo "threads: ${THREADS}"
	echo "seconds: ${SECONDS_EACH}"
	echo "load_costs: ${LOAD_COSTS:-as in the sources} (from ${costs_from})"
} > "${dir}/meta.txt"
echo "directory: ${dir}"

# Of the table of the threads ($2: "threads" for the cycle, "graph" for
# the graph in the cycles), over its rows: $3 the column, $4 "max" or
# "sum".
over_threads() {
	awk -v table="$2" -v column="$3" -v how="$4" '
		table == "threads" && /^slot +kind +end/ { inside = 1; next }
		table == "graph" && /^slot +kind +nodes_run/ { inside = 1; next }
		inside && ($1 !~ /^[0-9]+$/ || NF == 0) { inside = 0 }
		inside && $column != "-" {
			if (how == "sum") value += $column
			else if (!seen || $column + 0 > value + 0) value = $column
			seen = 1
		}
		END { if (seen) print value; else print "-" }' "$1"
}

row='%-8s %12s %12s %12s %9s %10s %6s %7s %9s %9s %8s  %s\n'
first=
{
	echo "load_costs: ${LOAD_COSTS:-as in the sources} (from ${costs_from})"
	# shellcheck disable=SC2059  # the format is in a variable on purpose
	printf "${row}" "threads" "expected_us" "slowest_avg" "slowest_max" "speed_up" "critical" "cut" "missed" "waits" "wait_max" "starved" "graph"
	for threads in ${THREADS}; do
		out="${dir}/threads-${threads}.txt"
		# shellcheck disable=SC2086  # ENGINE_OPTIONS is several words
		"${ENGINE}" run --graph "${GRAPH}" --rate "${RATE}" --threads "${threads}" \
			--seconds "${SECONDS_EACH}" ${ENGINE_OPTIONS} ${LOAD_COSTS:+--load-costs ${LOAD_COSTS}} \
			> "${out}" 2> "${dir}/threads-${threads}.err"
		slowest=$(over_threads "${out}" threads 10 max)
		first=${first:-${slowest}}
		# shellcheck disable=SC2059  # the format is in a variable on purpose
		printf "${row}" "${threads}" \
			"$(field "${out}" graph_makespan_us | sed 's/ .*//')" \
			"${slowest}" \
			"$(over_threads "${out}" threads 11 max)" \
			"$(awk -v first="${first}" -v this="${slowest}" 'BEGIN {
				if (this + 0 > 0) printf "%.2f", first / this; else print "-" }')" \
			"$(field "${out}" graph_critical_path_measured_us | sed 's/ .*//')" \
			"$(over_threads "${out}" threads 6 sum)" \
			"$(over_threads "${out}" threads 5 sum)" \
			"$(over_threads "${out}" graph 4 sum)" \
			"$(over_threads "${out}" graph 6 max)" \
			"$(over_threads "${out}" graph 8 sum)" \
			"$(field "${out}" graph | cut -c1-80)"
	done
} | tee "${dir}/summary.txt"
{
	echo
	echo "columns: expected_us, how long the compiler expects a cycle to last on so many threads;"
	echo "  slowest_avg and slowest_max, the compute phase of the thread where it was longest, on"
	echo "  average and at the most; speed_up, slowest_avg of the first row over that of this one;"
	echo "  critical, the longest chain of nodes that wait for each other, as its nodes took; cut,"
	echo "  cycles ended by the compute budget, all threads; missed, periods gone by unserved;"
	echo "  waits, nodes a thread had to wait for, and wait_max the longest of those waits;"
	echo "  starved, nodes whose inputs never came. Microseconds."
	echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} | tee -a "${dir}/summary.txt"
