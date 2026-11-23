#!/bin/sh
# cycle-record.sh — what the record of every cycle costs, and what it
# says with four live threads (step P1.6a).
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/cycle-record.sh
#
# Two measurements, both with the engine of the image:
#
#   cost    the engine on the sound card at 96 kHz, three times with the
#           record and three times without it (--no-record), in turn.
#           The figures compared are those each live thread sums up by
#           itself, which are there in both cases: how long the compute
#           phase lasts and how late the thread wakes.
#   four    four live threads on the timer (no card: with a card the
#           engine has one live thread, until step P1.6b), each busy for
#           60 % of the period. The record says, thread by thread, how
#           late the wake-ups are and which cycles are to be looked at.
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

if [ -n "${identifier}" ]; then
	echo
	echo "== cost: the card at 96 kHz, ${COST_SECONDS} s after ${WARM_UP} s of warm-up, with and without the record =="
	# The interface needs some seconds after a change of rate.
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate 96000 --seconds 8 \
		--allow-xruns --require-rt > "${dir}/cost-settle.txt" 2>&1
	round=1
	while [ "${round}" -le 3 ]; do
		for record in with without; do
			option=
			[ "${record}" = without ] && option=--no-record
			# shellcheck disable=SC2086  # option is empty or one word
			"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate 96000 \
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

{
	echo
	echo "== four: four live threads on the timer, 60 % busy, ${FOUR_SECONDS} s =="
	"${ENGINE}" run --backend null --threads 4 --load 60 --seconds "${FOUR_SECONDS}" --require-rt \
		> "${dir}/four.txt" 2>&1
	grep -E '^(record_of_every_cycle|record, slot|  wake-up|  period, off|  wake_up_bands|  least_margin_us|cycles_to_look_at)' "${dir}/four.txt"
	grep -A 2 -E '^  slot [0-9]+.* cycle ' "${dir}/four.txt" | head -n 40
} | tee "${dir}/four-summary.txt"

echo
echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "${dir}/meta.txt"
