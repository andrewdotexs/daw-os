#!/bin/sh
# cycle-record.sh — what the record of every cycle costs, and what it
# says with four live threads (step P1.6a).
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/cycle-record.sh
#
# Three measurements, all with the engine of the image; PARTS chooses
# among them (default: all three):
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

echo
echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "${dir}/meta.txt"
