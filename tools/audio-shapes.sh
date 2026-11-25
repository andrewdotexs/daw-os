#!/bin/sh
# audio-shapes.sh — does a shape of the buffers of the sound card hold
# when the live threads have work to do? (step P1.12c.)
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/audio-shapes.sh
#
# dawos-latency-test measures how long the way round is with each shape
# (frames in a period, periods in a buffer), with threads that have
# nothing to do: they hand the output over a few microseconds after the
# wake-up. But at a wake-up the card has only so much left to play, and
# it is less the fewer the periods; a thread that computes for most of
# the period hands the output over late. Whether the card is still
# playing by then is what this measures.
#
# For each case of CASES, written FRAMES:PERIODS:LOAD (frames of a period
# at 44.1 and 48 kHz, twice as many at 88.2 and 96 kHz, four times above;
# LOAD the share of the period every live thread is kept busy, in per
# cent), the engine runs on the card at RATE for JUDGED seconds after a
# minute of warm-up, with a tone through the cable (output 3 to input 3)
# and the count inside the interface (output 5 to input 5).
#
# JUDGED is 400 seconds by default: the frames ready at a wake-up drift
# with the difference between the clock of the interface and the one of
# the USB bus, and on the Scarlett 18i20 of the fourth generation they go
# through all their values in about 330 seconds (open point A-68).
#
# To try the script itself where there is no interface (a virtual
# machine), DEVICE_OPTIONS replaces the USB card with the options of the
# engine it holds, and the router is left alone.
#
# Results go to /var/lib/dawos/measure/shapes-<time>[-LABEL].

set -u

label=${1:-}
ENGINE=${DAWOS_ENGINE:-dawos-engine}
SETUP=${DAWOS_CARD_SETUP:-dawos-card-setup}
CARDS_DIR=${DAWOS_CARDS_DIR:-/usr/share/dawos/cards}
BASE=/var/lib/dawos/measure
RATE=${RATE:-48000}
CASES=${CASES:-96:2:60 64:3:60 64:3:85 96:3:85}
JUDGED=${JUDGED:-400}
WARM_UP=${WARM_UP:-60}
DEVICE_OPTIONS=${DEVICE_OPTIONS:-}

die() {
	echo "audio-shapes: $*" >&2
	exit 1
}

field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

[ "$(id -u)" = 0 ] || die "must run as root"
case "${RATE}" in
44100 | 48000) factor=1 ;;
88200 | 96000) factor=2 ;;
176400 | 192000) factor=4 ;;
*) die "rate ${RATE}: not one of the six of the engine" ;;
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
	device="--device hw:${identifier},0"
else
	device=${DEVICE_OPTIONS}
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/shapes-${stamp}${label:+-${label}}"
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
	echo "cases: ${CASES}"
	echo "seconds: ${JUDGED} after ${WARM_UP} of warm-up"
} > "${dir}/meta.txt"
echo "directory: ${dir}"

# The router of the interface: the known state, then the loop inside.
loop="${CARDS_DIR}/${model}.test-loop.conf"
if [ -n "${DEVICE_OPTIONS}" ]; then
	:
elif [ -r "${CARDS_DIR}/${model}.conf" ] && [ -r "${loop}" ]; then
	{
		"${SETUP}" apply --card "${card}"
		"${SETUP}" apply --card "${card}" "${loop}"
	} > "${dir}/routing-before.txt" 2>&1
else
	die "no routing files for the card ${model}: the count needs the loop inside the interface"
fi

# Of the thread that holds the card (slot 0), from the record of every
# cycle: the cycles ended by the compute budget, the wake-ups of 20
# microseconds and more, the cycles that began behind (their period was
# due before the cycle before had ended: no sleep, not a wake-up), the
# least time left before the instant a cycle had to end by, the frames
# ready and the frames still to play at a wake-up.
slot0() {
	awk -v what="$2" '
		/^record, slot 0:/ { inside = 1; next }
		/^record, / { inside = 0 }
		inside && what == "cut" && $1 == "periods_gone_by:" { sub(";", "", $8); print $8 }
		inside && what == "late" && $1 == "wake_up_bands:" { sub(",", "", $17); print $17 + $21 }
		inside && what == "behind" && $1 == "began_behind:" { print $2 }
		inside && what == "margin" && $1 == "least_margin_us:" { print $2 }
		inside && what == "ready" && $1 == "card_frames_at_the_wake_up:" { sub(",", "", $5); print $3 "-" $5 }
		inside && what == "to_play" && $1 == "card_frames_at_the_wake_up:" { print $8 "-" $10 }' "$1" | head -n 1
}

row='%-10s %5s  %-26s %-20s %6s %6s %7s %10s %9s %9s  %s\n'

# One case: $1 frames of a period at 48 kHz, $2 periods, $3 load.
run() {
	name="$1x$2-load$3"
	# shellcheck disable=SC2086  # device is several words
	"${ENGINE}" run --backend alsa ${device} --rate "${RATE}" --quantum "$(($1 * factor))" --periods "$2" \
		--load "$3" --seconds "$((JUDGED + WARM_UP))" --warm-up "${WARM_UP}" --require-rt \
		--tone 3 --tone-cycles 3 --listen 3 --count 5:5 \
		> "${dir}/${name}.txt" 2> "${dir}/${name}.err"
	# shellcheck disable=SC2059  # the format is in a variable on purpose
	printf "${row}" "$1:$2" "$3" \
		"$(field "${dir}/${name}.txt" card_xruns | sed 's/ of them//')" \
		"$(field "${dir}/${name}.txt" count_slips_on_input_5 | sed 's/;.*//; s/ samples//')" \
		"$(slot0 "${dir}/${name}.txt" cut)" \
		"$(slot0 "${dir}/${name}.txt" late)" \
		"$(behind=$(slot0 "${dir}/${name}.txt" behind) && echo "${behind:-0}")" \
		"$(slot0 "${dir}/${name}.txt" margin)" \
		"$(slot0 "${dir}/${name}.txt" ready)" \
		"$(slot0 "${dir}/${name}.txt" to_play)" \
		"$(field "${dir}/${name}.txt" audio | cut -c1-70)"
}

{
	# shellcheck disable=SC2059  # the format is in a variable on purpose
	printf "${row}" "shape" "load" "xruns" "slips of the count" "cut" "late" "behind" "left_us" "ready" "to_play" "audio"
	# The interface needs some seconds after a change of rate, and the
	# streams of the runs that count must start afresh.
	# shellcheck disable=SC2086  # device is several words
	"${ENGINE}" run --backend alsa ${device} --rate "${RATE}" --seconds 8 --threads 1 \
		--allow-xruns --require-rt > "${dir}/settle.txt" 2>&1
	for case in ${CASES}; do
		frames=${case%%:*}
		rest=${case#*:}
		run "${frames}" "${rest%%:*}" "${rest#*:}"
	done
} | tee "${dir}/summary.txt"

[ -n "${DEVICE_OPTIONS}" ] || "${SETUP}" apply --card "${card}" > "${dir}/routing-after.txt" 2>&1
{
	echo
	echo "columns, all of the thread that holds the card: cut, its cycles ended by the compute"
	echo "  budget; late, its wake-ups of 20 us and more; behind, its cycles whose period was due"
	echo "  before the cycle before had ended; left_us, the least time left before the instant a"
	echo "  cycle had to end by (negative: after it); ready and to_play, the frames ready and the"
	echo "  frames the card still had to play at a wake-up, least and most."
	echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} | tee -a "${dir}/summary.txt"
