#!/bin/sh
# cycle-record.sh — what the record of every cycle costs, and what it
# says with four live threads (step P1.6a).
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/cycle-record.sh
#
# Four measurements, all with the engine of the image; PARTS chooses
# among them (default: the first three):
#
#   cost    the engine on the sound card at 96 kHz with one live thread,
#           three times with the record and three times without it
#           (--no-record), in turn. The figures compared are those each
#           live thread sums up by itself, which are there in both
#           cases: how long the compute phase lasts and how late the
#           thread wakes.
#   four    four live threads on the timer, each busy for 60 % of the
#           period. The record says, thread by thread, how late the
#           wake-ups are and which cycles are to be looked at.
#   card    the same four threads on the sound card at 96 kHz (step
#           P1.6b): the first holds the card, the others sleep on it and
#           follow its cycle. Twice: with nothing to do, and 60 % busy.
#           A tone goes out of output 3 and is listened to on input 3
#           (the cable of the audio tests): a slip of the signal shows.
#   entries who enters the kernel on the live CPUs (step P1.6c): the runs
#           of "four" and of "card" again, each once without the trace of
#           the kernel and once with it (--kernel-entries). The first
#           says what the trace costs; the second, phase by phase of the
#           cycle, the system calls, the interrupts and the threads of
#           the kernel, and how the wake-ups went. ENTRIES chooses
#           among its runs (default "timer card-0 card-60 card-one"):
#           timer, four threads on the timer; timer-here, the same with
#           the timers of the kernel kept on the CPU that sets them
#           (kernel.timer_migration at 0 for the run); card-0 and
#           card-60, four threads on the card; card-one, one thread.
#           Before and after every run the count of the interrupts of
#           the firmware (SMI) is read. The traced runs trace other CPUs
#           too, for the wake-ups told one by one: on the timer all of
#           them (the kernel may keep a timer on another CPU), on the
#           card those of the io domain (where its interrupt is served).
#
# Results go to /var/lib/dawos/measure/record-<time>[-LABEL].

set -u

label=${1:-}
ENGINE=${DAWOS_ENGINE:-dawos-engine}
BASE=/var/lib/dawos/measure
# Seconds of each run of "cost", after the warm-up, and of "four".
COST_SECONDS=${COST_SECONDS:-30}
FOUR_SECONDS=${FOUR_SECONDS:-60}
WARM_UP=8
PARTS=${PARTS:-cost four card}
ENTRIES=${ENTRIES:-timer card-0 card-60 card-one}

die() {
	echo "cycle-record: $*" >&2
	exit 1
}

[ "$(id -u)" = 0 ] || die "must run as root"
identifier=
for directory in /proc/asound/card[0-9]*; do
	[ -r "${directory}/usbid" ] || continue
	identifier=$(cat "${directory}/id")
	break
done

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/record-${stamp}${label:+-${label}}"
mkdir -p "${dir}" || die "cannot create ${dir}"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "engine: $("${ENGINE}" --version 2> /dev/null)"
	echo "card: ${identifier:-none}"
} > "${dir}/meta.txt"
echo "directory: ${dir}"

# Columns of the line of slot 0 in the table of the threads: 8 and 9 the
# wake-up (mean, longest), 10 and 11 the compute phase, 12 the least
# margin; microseconds.
thread_line() {
	awk '$1 == "0" && $2 == "first" {
		printf "compute mean %s longest %s; wake-up mean %s longest %s; least margin %s", $10, $11, $8, $9, $12
	}' "$1"
}

# What the record says of every thread, and of the threads together.
record_lines() {
	grep -E '^(audio|card_xruns|card_followers|slips_on_input_3|record_of_every_cycle|record, |  wake-up|  period, off|  first awake|  compute, whole|  wake_up_bands|  least_margin_us|  card_frames|cycles_to_look_at)' "$1"
	grep -A 2 -E '^  slot [0-9]+.* cycle ' "$1" | head -n 40
}

# What the trace of the kernel says, without the wake-ups told one by
# one: those stay in the file.
entries_lines() {
	awk '/^kernel_entries, wake-ups one by one/ { exit } /^kernel_entries/ { on = 1 } on' "$1"
}

# The figures a run with the trace is compared by with one without: of
# every live thread, the wake-up and the whole compute phase.
compared_lines() {
	grep -E '^(dawos-engine: |card_xruns|slips_on_input_3|record, slot|  wake-up +[0-9]|  wake_up_bands|  compute, whole|  first awake|  least_margin_us|cycles_to_look_at)' "$1"
}

wanted() {
	case " ${PARTS} " in
	*" $1 "*) return 0 ;;
	esac
	return 1
}

if ! wanted cost; then
	:
elif [ -n "${identifier}" ]; then
	echo
	echo "== cost: the card at 96 kHz, one live thread, ${COST_SECONDS} s after ${WARM_UP} s of warm-up, with and without the record =="
	# The interface needs some seconds after a change of rate.
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate 96000 --seconds 8 --threads 1 \
		--allow-xruns --require-rt > "${dir}/cost-settle.txt" 2>&1
	round=1
	while [ "${round}" -le 3 ]; do
		for record in with without; do
			option=
			[ "${record}" = without ] && option=--no-record
			# shellcheck disable=SC2086  # option is empty or one word
			"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate 96000 --threads 1 \
				--seconds "$((COST_SECONDS + WARM_UP))" --warm-up "${WARM_UP}" --require-rt ${option} \
				> "${dir}/cost-${record}-${round}.txt" 2>&1
			printf '%-8s %s: %s; %s\n' "${record}" "${round}" "$(thread_line "${dir}/cost-${record}-${round}.txt")" \
				"$(grep -E '^(record_of_every_cycle|card_xruns)' "${dir}/cost-${record}-${round}.txt" | tr '\n' ' ')"
		done
		round=$((round + 1))
	done
else
	echo "no USB sound card: the cost of the record is not measured"
fi | tee "${dir}/cost.txt"

if wanted four; then
	echo
	echo "== four: four live threads on the timer, 60 % busy, ${FOUR_SECONDS} s =="
	"${ENGINE}" run --backend null --threads 4 --load 60 --seconds "${FOUR_SECONDS}" --require-rt \
		> "${dir}/four.txt" 2>&1
	record_lines "${dir}/four.txt"
fi | tee "${dir}/four-summary.txt"

if ! wanted card; then
	:
elif [ -n "${identifier}" ]; then
	# The interface needs some seconds after a change of rate, and the
	# streams of the runs that count must start afresh (step P1.6a).
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate 96000 --seconds 8 --threads 1 \
		--allow-xruns --require-rt > "${dir}/card-settle.txt" 2>&1
	for load in 0 60; do
		echo
		echo "== card: four live threads on the card at 96 kHz, ${load} % busy, ${FOUR_SECONDS} s after ${WARM_UP} s of warm-up =="
		# With a tone through the cable from output 3 to input 3: a
		# signal that slips shows there, xrun or no xrun.
		"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate 96000 --threads 4 --load "${load}" \
			--seconds "$((FOUR_SECONDS + WARM_UP))" --warm-up "${WARM_UP}" --require-rt \
			--tone 3 --tone-cycles 3 --listen 3 \
			> "${dir}/card-${load}.txt" 2>&1
		record_lines "${dir}/card-${load}.txt"
	done
else
	echo "no USB sound card: the threads on the card are not measured"
fi | tee "${dir}/card-summary.txt"

# The interrupts of the firmware (SMI) the processor has counted since
# it was switched on: register 0x34 of CPU 0, where the kernel serves
# it. They stop every CPU, and no trace of the kernel sees them.
smi_count() {
	value=$(dd if=/dev/cpu/0/msr bs=8 skip=52 count=1 iflag=skip_bytes 2> /dev/null |
		od -An -tu8 | tr -d ' \n')
	echo "${value:-unknown}"
}

# One run of "entries": $1 name, $2 words, the rest the options of the
# engine. Twice: without the trace and with it.
entries_run() {
	name=$1
	words=$2
	shift 2
	for trace in plain traced; do
		option=
		with="without"
		if [ "${trace}" = traced ]; then
			case "${name}" in
			timer*) option="--kernel-entries --trace-also all" ;;
			*) option="--kernel-entries --trace-also io" ;;
			esac
			with="with"
		fi
		echo
		echo "== entries: ${words}, ${FOUR_SECONDS} s; ${with} the trace of the kernel =="
		before=$(smi_count)
		# shellcheck disable=SC2086  # option is empty or several words
		"${ENGINE}" run "$@" --require-rt ${option} > "${dir}/entries-${name}-${trace}.txt" 2>&1
		echo "smi_count: ${before} before, $(smi_count) after; uptime_s at the end: $(cut -d. -f1 /proc/uptime)"
		compared_lines "${dir}/entries-${name}-${trace}.txt"
		[ "${trace}" = traced ] && entries_lines "${dir}/entries-${name}-${trace}.txt"
	done
}

# Is $1 among the runs of "entries" asked for?
entries_wanted() {
	case " ${ENTRIES} " in
	*" $1 "*) return 0 ;;
	esac
	return 1
}

if wanted entries; then
	if entries_wanted timer; then
		entries_run timer "four live threads on the timer, 60 % busy" \
			--backend null --threads 4 --load 60 --seconds "${FOUR_SECONDS}"
	fi
	if entries_wanted timer-here; then
		migration=$(cat /proc/sys/kernel/timer_migration 2> /dev/null)
		echo
		echo "kernel.timer_migration: ${migration:-not there}; 0 for the next two runs"
		[ -n "${migration}" ] && echo 0 > /proc/sys/kernel/timer_migration
		entries_run timer-here "four live threads on the timer, 60 % busy, timers kept on their CPU" \
			--backend null --threads 4 --load 60 --seconds "${FOUR_SECONDS}"
		[ -n "${migration}" ] && echo "${migration}" > /proc/sys/kernel/timer_migration
	fi
	if [ -z "${identifier}" ]; then
		echo "no USB sound card: the kernel on the live CPUs is looked at on the timer only"
	else
		case " ${ENTRIES} " in
		*" card-"*)
			"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate 96000 --seconds 8 --threads 1 \
				--allow-xruns --require-rt > "${dir}/entries-settle.txt" 2>&1
			;;
		esac
		for load in 0 60; do
			entries_wanted "card-${load}" || continue
			entries_run "card-${load}" "four live threads on the card at 96 kHz, ${load} % busy" \
				--backend alsa --device "hw:${identifier},0" --rate 96000 --threads 4 --load "${load}" \
				--seconds "$((FOUR_SECONDS + WARM_UP))" --warm-up "${WARM_UP}" \
				--tone 3 --tone-cycles 3 --listen 3
		done
		if entries_wanted card-one; then
			entries_run card-one "one live thread on the card at 96 kHz, nothing to do" \
				--backend alsa --device "hw:${identifier},0" --rate 96000 --threads 1 \
				--seconds "$((FOUR_SECONDS + WARM_UP))" --warm-up "${WARM_UP}" \
				--tone 3 --tone-cycles 3 --listen 3
		fi
	fi
	# A run that was killed leaves its trace on: take it away.
	[ -d /sys/kernel/tracing/instances/dawos-engine ] &&
		rmdir /sys/kernel/tracing/instances/dawos-engine 2> /dev/null
fi | tee "${dir}/entries-summary.txt"

echo
echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "${dir}/meta.txt"
