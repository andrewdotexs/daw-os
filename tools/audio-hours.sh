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
# end of every case. With REQUIRE_CAMPAIGN=1 the hours do not start
# unless the campaign is running: for an acceptance, where an hour
# begun before the load is an hour to do again.
#
# With ASAP=1 (or 0) the hours turn on (or off) the switch that the
# kernel of step P1.8c gives the USB driver, iso_asap: the output
# transfers handed to the controller as "as soon as possible". It is
# put back when the hours end. A transfer handed over late then leaves
# a hole on the bus, and the output is that much later for good: so in
# every case the way round of the count must be the same at the end as
# at the start, and no sample of it out of place (step P1.8d).
#
# That judgement does not see everything: a transfer that goes one
# microframe late is taken up by the queue of the interface, and the
# way round stays as it was (step P1.8e). What it leaves is on the bus:
# from then on an output transfer stays with the controller 125 us
# longer. So each case reads the bus for two seconds, twenty seconds
# after its start and fifteen before its end, and the summary says by
# how much the two differ (step P1.8f). It is shown, not judged. The
# second reading falls in the judged time: BUS_LOOK=0 leaves both out.
#
# A case passes when: the engine says "audio: PASS" and "compute: PASS";
# no xrun; no slip of the count; no sample of it out of place; the same
# way round at the end as at the start; no record of a cycle lost; no page of
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
REQUIRE_CAMPAIGN=${REQUIRE_CAMPAIGN:-}
ASAP=${ASAP:-}
SWITCH=/sys/module/snd_usb_audio/parameters/iso_asap
# Whether the USB bus is read for two seconds near the start and near
# the end of each case (step P1.8f). 0: not read.
BUS_LOOK=${BUS_LOOK:-1}

usage() {
	cat << USAGE
Usage:
  ${PROG} --start [LABEL]   start the hours in the background
  ${PROG} --status          what is running, or the last summary
  ${PROG} --stop            stop the hours that are running

In the environment of --start: RATE (default 96000), SECONDS_JUDGED
(default 3600), CASES (default "passthrough loaded"; also "swap"),
REQUIRE_CAMPAIGN (set: do not start unless the campaign is running),
ASAP (0 or 1: the switch iso_asap of the USB driver, for these hours),
BUS_LOOK (0: do not read the USB bus near the start and the end).
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
	samples=$(sed -n 's/^count_samples_on_input_[0-9]*: //p' "$1" | head -n 1)
	case "${samples}" in
	*" in place, 0 out of place"*) ;;
	*) why="${why}; samples of the count: ${samples:-not counted}" ;;
	esac
	# The way round, as found at the start and as it is at the end.
	round=$(sed -n 's/^count_on_input_[0-9]*: way round \([0-9]*\) samples.*at the end \([0-9]*\) samples.*/\1 \2/p' "$1" | head -n 1)
	case "${round}" in
	"") why="${why}; the way round of the count was not found" ;;
	*) [ "${round% *}" = "${round#* }" ] || why="${why}; the way round went from ${round% *} to ${round#* } samples" ;;
	esac
	if [ "${4:-}" = swap ]; then
		case "$(field "$1" graph)" in
		PASS*) ;;
		*) why="${why}; graph: $(field "$1" graph | cut -c1-160)" ;;
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

# ---- The USB bus of the card, read for two seconds: how long an
# output transfer stays with the controller, from handed over to back.
# With the switch iso_asap on a transfer is handed over a tenth of a
# millisecond before its turn. One that comes later than that goes a
# microframe (125 us) after its turn, and so does every transfer after
# it until the streams start again: the interface plays the same
# samples at the same instants from a shorter queue of its own, the way
# round of the count does not change, and nothing else shows it (step
# P1.8e). So the same reading near the start and near the end of a
# case tells how many microframes went by with no output in between.
# $1: the number of the card. Prints one line, or nothing where the
# card is not on a bus the kernel shows.
bus_look() {
	where=$(cat "/proc/asound/card$1/usbbus" 2> /dev/null) || return 0
	bus=$(echo "${where%%/*}" | sed 's/^0*//')
	[ -n "${bus}" ] || return 0
	[ -d /sys/kernel/debug/usb ] || mount -t debugfs debugfs /sys/kernel/debug 2> /dev/null
	monitor="/sys/kernel/debug/usb/usbmon/${bus}u"
	[ -r "${monitor}" ] || return 0
	# The reader goes ahead of ordinary work: lines it does not take in
	# time are lost. They are kept in memory, not on the disk.
	chrt -f 10 cat "${monitor}" > "${RUN}/bus.raw" 2> /dev/null &
	bus_reader=$!
	echo "${bus_reader}" > "${RUN}/bus-reader.pid"
	sleep 2
	kill "${bus_reader}" 2> /dev/null
	wait "${bus_reader}" 2> /dev/null
	rm -f "${RUN}/bus-reader.pid"
	awk -v device=":${where##*/}:" '
		$4 ~ /^Zo:/ && index($4, device) {
			if ($3 == "S") sent[$1] = $2
			else if ($3 == "C" && ($1 in sent)) {
				stay = $2 - sent[$1]
				delete sent[$1]
				times[int(stay / 5)]++
				total++
				if (least == "" || stay < least) least = stay
				if (stay > most) most = stay
			}
		}
		END {
			if (total == 0) { print "no output transfer of the card was read"; exit }
			# The middle one, in steps of 5 us.
			for (step = int(least / 5); step <= int(most / 5); step++) {
				seen += times[step]
				if (seen * 2 >= total) break
			}
			printf "half %d us (least %d, most %d, %d transfers in two seconds)\n", step * 5, least, most, total
		}' "${RUN}/bus.raw"
	rm -f "${RUN}/bus.raw"
}

# The two readings of a case, as the lines of the summary. $1 and $2:
# the files they were written to.
bus_lines() {
	at_start=$(cat "$1" 2> /dev/null)
	at_end=$(cat "$2" 2> /dev/null)
	[ -n "${at_start}${at_end}" ] || return 0
	echo "bus_output_with_the_controller: at the start ${at_start:-not read}; at the end ${at_end:-not read}"
	first=$(echo "${at_start}" | sed -n 's/^half \([0-9]*\) us.*/\1/p')
	last=$(echo "${at_end}" | sed -n 's/^half \([0-9]*\) us.*/\1/p')
	if [ -n "${first}" ] && [ -n "${last}" ]; then
		echo "bus_output_later_by: $((last - first)) us from the start to the end, $(((last - first + (last >= first ? 62 : -62)) / 125)) microframes (shown, not judged)"
	fi
}

# What a case says, for the summary.
lines_of() {
	grep -E '^(syscall_dispatch|syscalls_stopped_in_compute|card_xruns|card_recoveries|card_short_writes|card_start_top_ups|card_first_start_[0-9]+|card_followers|slips_on_input_3|count_on_input_[0-9]+|count_slips_on_input_[0-9]+|count_samples_on_input_[0-9]+|graph_file|graph_swap_file|graph_changes|graph_change_[a-z_]+|graph_changes_seen|graph_refused|graph_critical_path_measured_us|graph: |record_of_every_cycle|record, |  period, off|  wake-up +[0-9]|  input and patch|  nodes |  output |  compute, whole|  first awake|  wake_up_bands|  least_margin_us|  card_frames|  periods_gone_by|cycles_to_look_at|card_told_late_by_minute|audio: |compute: )' "$1"
	# Every slip of the count with its instant, the first twenty: with
	# the switch iso_asap on they tell when the output fell behind.
	grep -E '^ *count slip on input ' "$1" | head -n 20
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

	# The switch of the USB driver, for all the cases; put back at the
	# end, whatever was asked.
	switch_before=
	if [ -n "${ASAP}" ]; then
		switch_before=$(cat "${SWITCH}")
		echo "${ASAP}" > "${SWITCH}"
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
		engine_pid=$!
		echo "${engine_pid}" > "${RUN}/engine.pid"
		# The bus, twenty seconds after the start (in the warm-up) and
		# fifteen before the end.
		looker=
		rm -f "${dir}/${name}-bus-start.txt" "${dir}/${name}-bus-end.txt"
		if [ "${BUS_LOOK}" != 0 ] && [ -z "${DEVICE_OPTIONS}" ] && [ "$((SECONDS_JUDGED + WARM_UP))" -ge 60 ]; then
			(
				sleep 20
				bus_look "${card}" > "${dir}/${name}-bus-start.txt"
				sleep "$((SECONDS_JUDGED + WARM_UP - 20 - 2 - 15))"
				bus_look "${card}" > "${dir}/${name}-bus-end.txt"
			) &
			looker=$!
		fi
		wait "${engine_pid}"
		if [ -n "${looker}" ]; then
			# A case that ended early leaves the reader of the bus on.
			kill "${looker}" 2> /dev/null
			wait "${looker}" 2> /dev/null
			[ -r "${RUN}/bus-reader.pid" ] && kill "$(cat "${RUN}/bus-reader.pid")" 2> /dev/null
			rm -f "${RUN}/bus-reader.pid" "${RUN}/bus.raw"
		fi
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
			echo "iso_asap: $(cat "${SWITCH}" 2> /dev/null || echo "this kernel has no such switch")"
			echo "smi_count: ${before} before, ${after} after"
			echo "case_verdict: ${verdict}"
			bus_lines "${dir}/${name}-bus-start.txt" "${dir}/${name}-bus-end.txt"
			lines_of "${dir}/${name}.txt"
			[ -s "${dir}/${name}.err" ] && sed 's/^/stderr: /' "${dir}/${name}.err" | head -n 12
		} >> "${dir}/summary.txt"
	done

	[ -z "${switch_before}" ] || echo "${switch_before}" > "${SWITCH}"
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
if [ -n "${REQUIRE_CAMPAIGN}" ] && ! systemctl is-active --quiet dawos-campaign; then
	die "the campaign is not running, and REQUIRE_CAMPAIGN asks for it: start it with 'make campaign' first"
fi
command -v "${ENGINE}" > /dev/null || die "${ENGINE} is not in this image"
case "${ASAP}" in
'') ;;
0 | 1) [ -w "${SWITCH}" ] || die "this kernel has no switch iso_asap (${SWITCH}): it comes with step P1.8c" ;;
*) die "ASAP is 0 or 1" ;;
esac
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
	echo "iso_asap: ${ASAP:-left as it is: $(cat "${SWITCH}" 2> /dev/null || echo "this kernel has no such switch")}"
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
	--setenv=WARM_UP="${WARM_UP}" --setenv=DEVICE_OPTIONS="${DEVICE_OPTIONS}" --setenv=ASAP="${ASAP}" \
	--setenv=BUS_LOOK="${BUS_LOOK}" \
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
