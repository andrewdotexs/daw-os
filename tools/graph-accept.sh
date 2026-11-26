#!/bin/sh
# graph-accept.sh — criterion 7 of Phase 1, from the measures of step
# P1.7g: one verdict out of what three tools left.
#
# It runs on the development PC, after "make measure-fetch", and only
# reads: the files of the measures labelled LABEL (default p17g) in
# docs/misure/fase-0/raw.
#
#   tools/graph-accept.sh [LABEL]
#
# What the criterion asks, and what is judged here:
#
#   1. "the load calibrated against dawos-dspbench" (ADR 0011): from
#      tools/graph-calibrate.sh, the three classes with the figures of
#      the sources within 3 % of 10, 40 and 150 ns per frame at 96 kHz;
#      a stage of the engine against the same filter in dawos-dspbench
#      is reported, and must not be more than a fifth slower (open
#      point A-71);
#   2. "the same graph on 1, 2, 3 and 4 live CPUs, speed-up and
#      critical path measured": from tools/graph-threads.sh at 96 kHz,
#      the rate of the reference session, every row "PASS" (no cycle
#      cut, no period missed), the cycle within 10 % of what the
#      compiler expected, the critical path too, and a speed-up of 3.4
#      or more on four threads. At 48 and 192 kHz, where the load is
#      not calibrated (A-71), every row "PASS" and the figures
#      reported;
#   3. "the change of graph without a discontinuity in the signal":
#      from "make audio-hours" with CASES=swap, the verdict of the
#      case, which asks for a change every second, no sample of the
#      count out of place, no xrun, no slip, no system call in the
#      compute phase and no wake-up of 50 us or more; and the load of
#      the campaign running when the hour starts and when it ends.
#
# The thresholds are those written in the guide of the step before the
# measures were made.

set -eu

label=${1:-p17g}
here=$(cd "$(dirname "$0")/.." && pwd)
raw="${here}/docs/misure/fase-0/raw"

CLASS_TOLERANCE=3
SCHEDULE_TOLERANCE=10
STAGE_RATIO_LIMIT=1.20
SPEED_UP_LEAST=3.40

work=$(mktemp)
trap 'rm -f "${work}"' EXIT

failed=0
fail() {
	failed=$((failed + 1))
	echo "  FAIL: $*"
}

# The newest directory of kind $1 with the label, or nothing.
newest() {
	found=
	for candidate in "${raw}/$1"-*-"${label}"; do
		[ -d "${candidate}" ] && found=${candidate}
	done
	echo "${found}"
}

echo "criterion 7 of Phase 1, from the measures labelled '${label}'"

# ---- 1. The load.
echo
echo "1. the reference load, calibrated"
calibrate=$(newest calibrate)
if [ -z "${calibrate}" ]; then
	fail "no calibration: tools/graph-calibrate.sh was not run with this label, or not fetched"
else
	echo "  from ${calibrate##*/} (image $(sed -n 's/^image: //p' "${calibrate}/meta.txt"))"
	classes=$(sed -n 's/^the_classes_with_the_figures_of_the_sources: //p' "${calibrate}/summary.txt")
	echo "  the classes with the figures of the sources: ${classes:-not there} ns per frame"
	echo "${classes}" | awk -v tolerance="${CLASS_TOLERANCE}" '
		{
			gsub(",", "")
			wrong = 0
			wanted["light"] = 10; wanted["medium"] = 40; wanted["heavy"] = 150
			for (field = 1; field < NF; field += 2) {
				off = ($(field + 1) - wanted[$field]) / wanted[$field] * 100
				printf "    %-6s %8.3f against %3d: %+.1f %%\n", $field, $(field + 1), wanted[$field], off
				if (off > tolerance || off < -tolerance) wrong = 1
				seen++
			}
			exit (wrong || seen != 3)
		}' || fail "a class is more than ${CLASS_TOLERANCE} % from what it declares"
	stage=$(sed -n 's/^load_costs: [0-9.]*,\([0-9.]*\),.*/\1/p' "${calibrate}/summary.txt")
	bench=$(sed -n 's/^dspbench_on_cpu_[0-9]*: .*a vector of four: \([0-9.]*\).*/\1/p' "${calibrate}/summary.txt")
	if [ -n "${stage}" ] && [ -n "${bench}" ]; then
		ratio=$(awk -v stage="${stage}" -v bench="${bench}" 'BEGIN { printf "%.2f", stage / bench }')
		echo "  a stage: ${stage} ns per frame in the engine, ${bench} in dawos-dspbench: ${ratio} times (A-71)"
		awk -v ratio="${ratio}" -v limit="${STAGE_RATIO_LIMIT}" 'BEGIN { exit !(ratio <= limit) }' ||
			fail "a stage costs more than ${STAGE_RATIO_LIMIT} times the kernel of dawos-dspbench"
	else
		fail "the cost of a stage, or that of dawos-dspbench, is not in the summary"
	fi
fi

# ---- 2. The graph on one to four threads.
echo
echo "2. the same graph on one, two, three and four live threads"
rates_seen=
for directory in "${raw}"/graph-*-"${label}"; do
	[ -d "${directory}" ] || continue
	rate=$(sed -n 's/^rate: //p' "${directory}/meta.txt")
	rates_seen="${rates_seen} ${rate}"
	echo "  at ${rate} Hz, from ${directory##*/}:"
	awk -v judged="$([ "${rate}" = 96000 ] && echo 1 || echo 0)" \
		-v tolerance="${SCHEDULE_TOLERANCE}" -v least="${SPEED_UP_LEAST}" '
		$1 == "graph_critical_path_us:" { next }
		/^[1-4] / {
			rows++
			off = ($3 - $2) / $2 * 100
			printf "    %s thread%s: expected %7.2f us, measured %7.2f (%+.1f %%), speed-up %s, critical path %s us, %s\n",
				$1, ($1 == 1 ? " " : "s"), $2, $3, off, $5, $6, $12
			if ($12 != "PASS") wrong = wrong "; " $1 " threads: " $12
			if (judged && off > tolerance) wrong = wrong "; " $1 " threads: " sprintf("%.1f", off) " % over the schedule"
			if (judged && $1 == 4 && $5 + 0 < least) wrong = wrong "; speed-up on four threads " $5
		}
		END {
			if (rows != 4) wrong = wrong "; " rows " rows of four"
			if (wrong != "") { print "WRONG" wrong; exit 1 }
		}' "${directory}/summary.txt" > "${work}" || true
	grep -v '^WRONG' "${work}" || true
	wrong=$(sed -n 's/^WRONG; //p' "${work}")
	[ -z "${wrong}" ] || fail "${wrong}"
	if [ "${rate}" = 96000 ]; then
		# The critical path as the compiler expected it, against what
		# its nodes took on one thread.
		expected=$(sed -n 's/^graph_critical_path_us: \([0-9.]*\).*/\1/p' "${directory}/threads-1.txt" | head -n 1)
		measured=$(awk '$1 == 1 { print $6 }' "${directory}/summary.txt")
		if [ -n "${expected}" ] && [ -n "${measured}" ]; then
			echo "    critical path: expected ${expected} us, measured ${measured} on one thread"
			awk -v expected="${expected}" -v measured="${measured}" -v tolerance="${SCHEDULE_TOLERANCE}" \
				'BEGIN { exit !((measured - expected) / expected * 100 <= tolerance) }' ||
				fail "the critical path is more than ${SCHEDULE_TOLERANCE} % over what the compiler expected"
		else
			fail "the critical path is not in the files"
		fi
	fi
done
for rate in 48000 96000 192000; do
	case " ${rates_seen} " in
	*" ${rate} "*) ;;
	*) fail "no measure at ${rate} Hz: RATE=${rate} tools/graph-threads.sh was not run with this label, or not fetched" ;;
	esac
done

# ---- 3. The change of graph.
echo
echo "3. the change of graph, with the count going through it"
hours=$(newest hours)
if [ -z "${hours}" ]; then
	fail "no hours: make audio-hours CASES=swap was not run with this label, or not fetched"
else
	echo "  from ${hours##*/} (image $(sed -n 's/^image: //p' "${hours}/meta.txt"))"
	sed -n '/^== swap: /,$p' "${hours}/summary.txt" |
		grep -E '^(== swap|campaign: |smi_count: |case_verdict: |card_xruns: |count_on_input_|count_slips_on_input_|count_samples_on_input_|graph_changes: |graph_change_kept: |graph_change_passed_us: |graph_refused: |graph: |  wake_up_bands: )' |
		sed 's/^/  /' || true
	case "$(sed -n '/^== swap: /,$p' "${hours}/summary.txt" | sed -n 's/^case_verdict: //p' | head -n 1)" in
	PASS) ;;
	"") fail "the case swap has no verdict" ;;
	*) fail "the case swap did not pass" ;;
	esac
	# The hour is one under the load of the campaign, all of it.
	case "$(sed -n '/^== swap: /,$p' "${hours}/summary.txt" | sed -n 's/^campaign: //p' | head -n 1)" in
	"running at the start, running at the end") ;;
	*) fail "the campaign was not running for the whole hour: start it first, and look at 'make campaign' before starting the hour" ;;
	esac
fi

echo
if [ "${failed}" -eq 0 ]; then
	echo "criterion 7: PASS"
else
	echo "criterion 7: FAIL (${failed} things wrong)"
	exit 1
fi
