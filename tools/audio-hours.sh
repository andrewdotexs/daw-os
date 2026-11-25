#!/bin/sh
# audio-hours.sh — the acceptance of step P1.6: the engine on the sound
# card for an hour at a time, while the rest of the machine works.
#
# It runs ON the machine, in the background: "make audio-hours" copies
# this file to /var/lib/dawos and runs it there.
#
#   make audio-hours HOST=<address> DO="--start [LABEL]"
#   make audio-hours HOST=<address>                        (the status)
#   make audio-hours HOST=<address> DO=--stop
#
# What it does, at RATE (default 96000) with four live threads, the
# kernel stopping the system calls of the compute phase (step P1.6d)
# and the trace of the kernel on the live CPUs (step P1.6c):
#
#   passthrough  SECONDS (default 3600) after a minute of warm-up, the
#                threads with nothing to do but pass the sound;
#   loaded       the same with every thread busy for 60 % of the period;
#   swap         two graphs that take each other's place every second
#                (swap-a.graph and swap-b.graph, step P1.7e), the count
#                going through both: the acceptance of step P1.7g.
#
# CASES chooses among them (default: the first two, in this order). In
# all of them a tone goes out of output 3 and comes back on input 3
# through the cable, and a count goes out of output 5 and comes back
# inside the interface, through its router: on input 5 in the first
# two, on input 6 through the graph in the third. A signal that slips
# is found to the sample.
#
# The load of the rest of the machine is not started here: it is the one
# of the campaign of step P0.7, started before this with
#
#   make campaign HOST=<address> DO="--start load:8100 --label LABEL"
#
# and, from the development PC, "make campaign-net" for the network.
# Whether the campaign was running is written at the start and at the
# end of every case.
#
# A case passes when: the engine says "audio: PASS" and "compute: PASS";
# no xrun; no slip of the count; no record of a cycle lost; no page of
# the trace lost; no system call and no kernel entry in the compute
# phase that is not accounted for (they are listed: the verdict asks for
# no system call, and reports the entries); no wake-up of 50 us or more;
# no interrupt of the firmware (SMI). The case "swap" also asks for
# "graph: PASS", for a change of graph in every second, and for no
# sample of the count out of place.
#
# To try the script itself where there is no interface (a virtual
# machine), DEVICE_OPTIONS replaces the USB card with the options of the
# engine it holds, and the router is left alone:
#
#   DEVICE_OPTIONS="--device hw:Loopback,0,0 --capture-device hw:Loopback,1,0 --allow-xruns"
#
# Results: /var/lib/dawos/measure/hours-<time>[-LABEL]/.

set -u

PROG=audio-hours.sh
BASE=/var/lib/dawos/measure
UNIT=dawos-audio-hours
RUN=/run/dawos-audio-hours
ENGINE=${DAWOS_ENGINE:-dawos-engine}
SETUP=${DAWOS_CARD_SETUP:-dawos-card-setup}
CARDS_DIR=${DAWOS_CARDS_DIR:-/usr/share/dawos/cards}
GRAPHS=${DAWOS_GRAPHS:-/usr/share/dawos/graphs}
RATE=${RATE:-96000}
SECONDS_JUDGED=${SECONDS_JUDGED:-3600}
CASES=${CASES:-passthrough loaded}
WARM_UP=${WARM_UP:-60}
DEVICE_OPTIONS=${DEVICE_OPTIONS:-}

usage() {
	cat << USAGE
Usage:
  ${PROG} --start [LABEL]   start the hours in the background
  ${PROG} --status          what is running, or the last summary
  ${PROG} --stop            stop the hours that are running

In the environment of --start: RATE (default 96000), SECONDS_JUDGED
(default 3600), CASES (default "passthrough loaded"; also "swap").
USAGE
}

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

latest_dir() {
	latest=
	for candidate in "${BASE}"/hours-*/; do
		[ -d "${candidate}" ] && latest=${candidate%/}
	done
	echo "${latest}"
}

# The interrupts of the firmware the processor has counted since it was
# switched on (register 0x34 of CPU 0).
smi_count() {
	value=$(dd if=/dev/cpu/0/msr bs=8 skip=52 count=1 iflag=skip_bytes 2> /dev/null |
		od -An -tu8 | tr -d ' \n')
	echo "${value:-unknown}"
}

campaign_state() {
	if systemctl is-active --quiet dawos-campaign; then echo "running"; else echo "NOT running"; fi
}

# The wake-ups of 50 us and more of every thread of a run, summed.
late_wake_ups() {
	awk '/^  wake_up_bands:/ { sub(/.*50 and over /, ""); total += $1 } END { print total + 0 }' "$1"
}

# The verdict of one case, from the report of the engine: "PASS", or
# "FAIL" and why. $1 the report, $2 and $3 the SMI count before and
# after, $4 the name of the case.
judge() {
	why=
	case "$(field "$1" audio)" in
	PASS*) ;;
	*) why="${why}; audio: $(field "$1" audio | cut -c1-120)" ;;
	esac
	case "$(field "$1" compute)" in
	PASS*) ;;
	*) why="${why}; compute: $(field "$1" compute | cut -c1-120)" ;;
	esac
	case "$(field "$1" card_xruns)" in
	"0 "*) ;;
	*) why="${why}; xruns: $(field "$1" card_xruns)" ;;
	esac
	slips=$(sed -n 's/^count_slips_on_input_[0-9]*: //p' "$1" | head -n 1)
	case "${slips}" in
	"0 "*) ;;
	*) why="${why}; slips of the count: ${slips}" ;;
	esac
	if [ "${4:-}" = swap ]; then
		case "$(field "$1" graph)" in
		PASS*) ;;
		*) why="${why}; graph: $(field "$1" graph | cut -c1-160)" ;;
		esac
		samples=$(sed -n 's/^count_samples_on_input_[0-9]*: //p' "$1" | head -n 1)
		case "${samples}" in
		*" in place, 0 out of place"*) ;;
		*) why="${why}; samples of the count: ${samples}" ;;
		esac
		# One change a second, for all the seconds that are judged.
		changes=$(field "$1" graph_changes)
		made=${changes%% *}
		case "${made}" in
		"" | *[!0-9]*) made=0 ;;
		esac
		if [ "${made}" -lt "$((SECONDS_JUDGED - 5))" ]; then
			why="${why}; changes of graph: ${changes:-none}"
		fi
		case "${changes}" in
		*"then they stopped"*) why="${why}; the changes stopped: ${changes}" ;;
		esac
	fi
	case "$(field "$1" record_of_every_cycle)" in
	*" 0 dropped"*) ;;
	*) why="${why}; record: $(field "$1" record_of_every_cycle | cut -c1-60)" ;;
	esac
	case "$(field "$1" kernel_entries)" in
	*"lost before them: 0; pages not understood: 0;"*) ;;
	*) why="${why}; trace: $(field "$1" kernel_entries | cut -c1-160)" ;;
	esac
	case "$(field "$1" kernel_entries_in_compute)" in
	"0 system calls"*) ;;
	*) why="${why}; in compute: $(field "$1" kernel_entries_in_compute)" ;;
	esac
	late=$(late_wake_ups "$1")
	[ "${late}" = 0 ] || why="${why}; ${late} wake-ups of 50 us and more"
	[ "$2" = "$3" ] || why="${why}; SMI count from $2 to $3"
	if [ -z "${why}" ]; then echo "PASS"; else echo "FAIL${why}"; fi
}

# What a case says, for the summary.
lines_of() {
	grep -E '^(syscall_dispatch|syscalls_stopped_in_compute|card_xruns|card_recoveries|card_short_writes|card_followers|slips_on_input_3|count_on_input_[0-9]+|count_slips_on_input_[0-9]+|count_samples_on_input_[0-9]+|graph_file|graph_swap_file|graph_changes|graph_change_[a-z_]+|graph_changes_seen|graph_refused|graph_critical_path_measured_us|graph: |record_of_every_cycle|record, |  period, off|  wake-up +[0-9]|  input and patch|  nodes |  output |  compute, whole|  first awake|  wake_up_bands|  least_margin_us|  card_frames|  periods_gone_by|cycles_to_look_at|card_told_late_by_minute|audio: |compute: )' "$1"
	grep -A 2 -E '^  slot [0-9]+.* cycle [0-9]+ at ' "$1" | head -n 36
	awk '/^kernel_entries, wake-ups one by one/ { exit } /^kernel_entries/ { on = 1 } on' "$1"
}

run_all() {
	dir=$1
	card=$(field "${dir}/meta.txt" card_number)
	identifier=$(field "${dir}/meta.txt" card | sed 's/ .*//')
	model=$(field "${dir}/meta.txt" card | sed 's/.*(\(.*\))/\1/')
	if [ -n "${DEVICE_OPTIONS}" ]; then
		device=${DEVICE_OPTIONS}
	else
		device="--device hw:${identifier},0"
		loop="${CARDS_DIR}/${model}.test-loop.conf"
		{
			"${SETUP}" apply --card "${card}"
			"${SETUP}" apply --card "${card}" "${loop}"
		} > "${dir}/routing-before.txt" 2>&1
	fi

	echo "settling the interface at ${RATE} Hz" > "${dir}/now.txt"
	# shellcheck disable=SC2086  # device is several words
	"${ENGINE}" run --backend alsa ${device} --rate "${RATE}" --seconds 8 --threads 1 \
		--allow-xruns --require-rt > "${dir}/settle.txt" 2>&1

	passed=0
	cases=0
	for name in ${CASES}; do
		case "${name}" in
		passthrough)
			work="--load 0 --count 5:5"
			what="four live threads, 0 % busy"
			;;
		loaded)
			work="--load 60 --count 5:5"
			what="four live threads, 60 % busy"
			;;
		swap)
			work="--threads 4 --graph ${GRAPHS}/swap-a.graph --swap-graph ${GRAPHS}/swap-b.graph --swap-every 1 --count 5:6"
			what="four live threads, two graphs taking each other's place every second"
			;;
		*)
			echo "no case '${name}'" >> "${dir}/summary.txt"
			continue
			;;
		esac
		cases=$((cases + 1))
		[ -e "${RUN}/stop" ] && break
		before=$(smi_count)
		started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
		campaign_before=$(campaign_state)
		echo "${name}: started ${started}, $((SECONDS_JUDGED + WARM_UP)) s; the campaign was ${campaign_before}" > "${dir}/now.txt"
		# shellcheck disable=SC2086  # device and work are several words
		"${ENGINE}" run --backend alsa ${device} --rate "${RATE}" ${work} \
			--seconds "$((SECONDS_JUDGED + WARM_UP))" --warm-up "${WARM_UP}" --require-rt \
			--tone 3 --tone-cycles 3 --listen 3 --kernel-entries \
			> "${dir}/${name}.txt" 2> "${dir}/${name}.err" &
		echo $! > "${RUN}/engine.pid"
		wait $!
		after=$(smi_count)
		verdict=$(judge "${dir}/${name}.txt" "${before}" "${after}" "${name}")
		case "${verdict}" in
		PASS) passed=$((passed + 1)) ;;
		esac
		{
			echo
			echo "== ${name}: ${RATE} Hz, ${what}, ${SECONDS_JUDGED} s after ${WARM_UP} s of warm-up =="
			echo "from ${started} to $(date -u +%Y-%m-%dT%H:%M:%SZ); uptime_s at the end: $(cut -d. -f1 /proc/uptime)"
			echo "campaign: ${campaign_before} at the start, $(campaign_state) at the end"
			echo "smi_count: ${before} before, ${after} after"
			echo "case_verdict: ${verdict}"
			lines_of "${dir}/${name}.txt"
			[ -s "${dir}/${name}.err" ] && sed 's/^/stderr: /' "${dir}/${name}.err" | head -n 12
		} >> "${dir}/summary.txt"
	done

	[ -n "${DEVICE_OPTIONS}" ] || "${SETUP}" apply --card "${card}" > "${dir}/routing-after.txt" 2>&1
	# A run that was killed leaves its trace on: take it away.
	[ -d /sys/kernel/tracing/instances/dawos-engine ] &&
		rmdir /sys/kernel/tracing/instances/dawos-engine 2> /dev/null
	{
		echo
		if [ -e "${RUN}/stop" ]; then
			echo "verdict: STOPPED by hand (${passed} of ${cases} cases passed before)"
		elif [ "${passed}" -eq "${cases}" ] && [ "${cases}" -gt 0 ]; then
			echo "verdict: PASS (${passed} of ${cases} cases)"
		else
			echo "verdict: FAIL (${passed} of ${cases} cases)"
		fi
		echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	} >> "${dir}/summary.txt"
	rm -f "${dir}/now.txt" "${RUN}/engine.pid" "${RUN}/stop"
}

SELF=$(readlink -f "$0" 2> /dev/null)

case "${1:-}" in
-h | --help)
	usage
	exit 0
	;;
--status | "")
	last=$(latest_dir)
	if systemctl is-active --quiet "${UNIT}"; then
		echo "The hours are running: ${last}"
		[ -r "${last}/now.txt" ] && cat "${last}/now.txt"
		echo "now: $(date -u +%Y-%m-%dT%H:%M:%SZ); the campaign is $(campaign_state)"
		[ -r "${last}/summary.txt" ] && grep -E '^(== |case_verdict)' "${last}/summary.txt"
	elif [ -n "${last}" ]; then
		echo "No hours are running. Last ones: ${last}"
		if [ -r "${last}/summary.txt" ]; then cat "${last}/summary.txt"; else cat "${last}/meta.txt"; fi
	else
		echo "No hours are running, and none were run."
	fi
	exit 0
	;;
--stop)
	if ! systemctl is-active --quiet "${UNIT}"; then
		echo "${PROG}: no hours are running"
		exit 0
	fi
	# The engine is asked to end, so that the case is summed up; the
	# script then stops by itself.
	mkdir -p "${RUN}"
	: > "${RUN}/stop"
	[ -r "${RUN}/engine.pid" ] && kill "$(cat "${RUN}/engine.pid")" 2> /dev/null
	waited=0
	while systemctl is-active --quiet "${UNIT}" && [ "${waited}" -lt 30 ]; do
		sleep 1
		waited=$((waited + 1))
	done
	systemctl is-active --quiet "${UNIT}" && systemctl stop "${UNIT}" 2> /dev/null
	echo "${PROG}: stopped"
	exit 0
	;;
--run)
	[ -n "${2:-}" ] || die "--run needs the result directory"
	run_all "$2"
	exit 0
	;;
--start)
	label=${2:-}
	;;
*)
	usage >&2
	exit 1
	;;
esac

[ "$(id -u)" = 0 ] || die "must run as root"
if [ -z "${SELF}" ] || [ ! -r "${SELF}" ]; then
	die "run it from a file: make audio-hours copies it to the machine"
fi
systemctl is-active --quiet "${UNIT}" && die "the hours are already running"
command -v "${ENGINE}" > /dev/null || die "${ENGINE} is not in this image"
card=
identifier=none
model=none
if [ -z "${DEVICE_OPTIONS}" ]; then
	for directory in /proc/asound/card[0-9]*; do
		[ -r "${directory}/usbid" ] || continue
		card=${directory##*/card}
		break
	done
	[ -n "${card}" ] || die "no USB sound card: is the interface connected?"
	identifier=$(cat "/proc/asound/card${card}/id")
	model=$(tr ':' '-' < "/proc/asound/card${card}/usbid")
	if [ ! -r "${CARDS_DIR}/${model}.conf" ] || [ ! -r "${CARDS_DIR}/${model}.test-loop.conf" ]; then
		die "no routing files for the card ${model}: the count needs the loop inside the interface"
	fi
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/hours-${stamp}${label:+-${label}}"
mkdir -p "${dir}" "${RUN}" || die "cannot create ${dir}"
rm -f "${RUN}/stop"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "engine: $("${ENGINE}" --version 2> /dev/null)"
	echo "card: ${identifier} (${model})"
	echo "card_number: ${card}"
	echo "rate: ${RATE}"
	echo "seconds: ${SECONDS_JUDGED} after ${WARM_UP} of warm-up"
	echo "cases: ${CASES}"
	[ -n "${DEVICE_OPTIONS}" ] && echo "device_options: ${DEVICE_OPTIONS}"
	echo "timer_migration: $(cat /proc/sys/kernel/timer_migration 2> /dev/null)"
	echo "uptime_s: $(cut -d. -f1 /proc/uptime)"
	echo "campaign: $(campaign_state)"
} > "${dir}/meta.txt"
: > "${dir}/summary.txt"

# The hours run from a copy of their own: this file may be replaced by
# a newer one while they run, and a shell reads its script as it goes.
copy="${RUN}/audio-hours-${stamp}.sh"
cp "${SELF}" "${copy}" || die "could not copy ${SELF} to ${RUN}"
systemd-run --quiet --unit="${UNIT}" --collect \
	--description="dawOS engine, acceptance hours (P1.6e)" \
	--setenv=RATE="${RATE}" --setenv=SECONDS_JUDGED="${SECONDS_JUDGED}" --setenv=CASES="${CASES}" \
	--setenv=WARM_UP="${WARM_UP}" --setenv=DEVICE_OPTIONS="${DEVICE_OPTIONS}" \
	/bin/sh "${copy}" --run "${dir}" ||
	die "could not start the hours"

total=0
for name in ${CASES}; do
	total=$((total + SECONDS_JUDGED + WARM_UP + 5))
done
echo "Hours started: ${CASES}, at ${RATE} Hz"
echo "They take about $(((total + 30) / 60)) minutes. The campaign is $(campaign_state)."
echo "Results in ${dir}"
echo "Follow them with: make audio-hours HOST=<address>"
