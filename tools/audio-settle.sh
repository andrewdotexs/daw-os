#!/bin/sh
# audio-settle.sh — does the signal slip at one rate because the
# interface has just changed rate, because of the record of every cycle,
# or by itself? (step P1.6a, after a run of ten minutes at 96 kHz with
# three slips of about a millisecond and no xrun.)
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/audio-settle.sh
#
# Runs at RATE (default 96000), each of JUDGED seconds (default 600)
# after a minute of warm-up, with a tone through the cable (output 3 to
# input 3) and the count inside the interface (output 5 to input 5,
# through its router), which gives every slip with its exact size and
# sign:
#
#   changed          right after a change of rate (ten seconds at
#                    OTHER, default 48000, come first), with the record;
#   changed-norecord the same, without the record (--no-record);
#   settled          once more with the record, the interface having
#                    been at RATE all the time since the run before;
#   after-xrun       like "settled", with an xrun made on purpose ten
#                    seconds after the warm-up: are streams started
#                    again after an xrun short, as after a change of
#                    rate? (the engine does not judge the slips of a run
#                    with a fault: the lines of the count say.)
#
# CASES chooses among them (default: all four, in this order).
#
# Results go to /var/lib/dawos/measure/settle-<time>[-LABEL].

set -u

label=${1:-}
ENGINE=${DAWOS_ENGINE:-dawos-engine}
SETUP=${DAWOS_CARD_SETUP:-dawos-card-setup}
CARDS_DIR=${DAWOS_CARDS_DIR:-/usr/share/dawos/cards}
BASE=/var/lib/dawos/measure
RATE=${RATE:-96000}
OTHER=${OTHER:-48000}
SECONDS_JUDGED=${JUDGED:-600}
WARM_UP=60
CASES=${CASES:-changed changed-norecord settled after-xrun}

die() {
	echo "audio-settle: $*" >&2
	exit 1
}

field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

[ "$(id -u)" = 0 ] || die "must run as root"
card=
for directory in /proc/asound/card[0-9]*; do
	[ -r "${directory}/usbid" ] || continue
	card=${directory##*/card}
	break
done
[ -n "${card}" ] || die "no USB sound card: is the interface connected?"
identifier=$(cat "/proc/asound/card${card}/id")
model=$(tr ':' '-' < "/proc/asound/card${card}/usbid")

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/settle-${stamp}${label:+-${label}}"
mkdir -p "${dir}" || die "cannot create ${dir}"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "engine: $("${ENGINE}" --version 2> /dev/null)"
	echo "card: ${identifier} (${model})"
	echo "rate: ${RATE}"
	echo "other_rate: ${OTHER}"
	echo "seconds: ${SECONDS_JUDGED} after ${WARM_UP} of warm-up"
	echo "cases: ${CASES}"
} > "${dir}/meta.txt"
echo "directory: ${dir}"

# The router of the interface: the known state, then the loop inside.
loop="${CARDS_DIR}/${model}.test-loop.conf"
if [ -r "${CARDS_DIR}/${model}.conf" ] && [ -r "${loop}" ]; then
	{
		"${SETUP}" apply --card "${card}"
		"${SETUP}" apply --card "${card}" "${loop}"
	} > "${dir}/routing-before.txt" 2>&1
else
	die "no routing files for the card ${model}: the count needs the loop inside the interface"
fi

# One run: $1 name, the rest are more options of the engine.
run() {
	name=$1
	shift
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate "${RATE}" \
		--seconds "$((SECONDS_JUDGED + WARM_UP))" --warm-up "${WARM_UP}" --require-rt \
		--tone 3 --tone-cycles 3 --listen 3 --count 5:5 --each-second "$@" \
		> "${dir}/${name}.txt" 2> "${dir}/${name}.err"
	echo
	echo "-- ${name} --"
	echo "audio: $(field "${dir}/${name}.txt" audio)"
	echo "xruns: $(field "${dir}/${name}.txt" card_xruns)"
	echo "count: $(field "${dir}/${name}.txt" count_on_input_5)"
	echo "count slips: $(field "${dir}/${name}.txt" count_slips_on_input_5)"
	grep 'count slip on input' "${dir}/${name}.txt"
	echo "slips of the tone: $(field "${dir}/${name}.txt" slips_on_input_3)"
	grep -E '^(card_told_late_by_minute|cycles_to_look_at)' "${dir}/${name}.txt"
}

# Away from the rate and back: what a session does when it changes rate.
change_rate() {
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate "${OTHER}" --seconds 10 \
		--allow-xruns --require-rt > "${dir}/$1-other-rate.txt" 2>&1
}

for name in ${CASES}; do
	case "${name}" in
	changed)
		change_rate changed
		run changed
		;;
	changed-norecord)
		change_rate changed-norecord
		run changed-norecord --no-record
		;;
	settled) run settled ;;
	after-xrun) run after-xrun --fault card-xrun --fault-after "$((WARM_UP + 10))" ;;
	*) echo "audio-settle: no case '${name}'" ;;
	esac
done | tee "${dir}/summary.txt"

"${SETUP}" apply --card "${card}" > "${dir}/routing-after.txt" 2>&1
echo
echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "${dir}/summary.txt"
