#!/bin/sh
# audio-margin.sh — how long before the end of the period must a live
# thread stop taking work, for the sound card to get its output in time?
# (step P1.6e, open point A-54.)
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/audio-margin.sh
#
# The engine runs on the card with four live threads that have more work
# than fits in a period (LOAD, default 120 % of it), so that in every
# cycle it is the compute budget that ends the work: no node is started
# later than MARGIN microseconds before the end of the period (ADR 0021,
# point 4). The run is repeated with the margins of MARGINS, from the
# widest down, at each rate of RATES. A margin that is too narrow shows
# as xruns: the output reaches the driver after the card has played what
# it had. With a tone through the cable (output 3 to input 3) and the
# count inside the interface (output 5 to input 5), which tells a signal
# that slips to the sample.
#
# At the end one more run for each rate, with work that fits (60 %) and
# the default margin: the reference.
#
# To try the script itself where there is no interface (a virtual
# machine), DEVICE_OPTIONS replaces the USB card with the options of the
# engine it holds, and the router is left alone.
#
# Results go to /var/lib/dawos/measure/margin-<time>[-LABEL].

set -u

label=${1:-}
ENGINE=${DAWOS_ENGINE:-dawos-engine}
SETUP=${DAWOS_CARD_SETUP:-dawos-card-setup}
CARDS_DIR=${DAWOS_CARDS_DIR:-/usr/share/dawos/cards}
BASE=/var/lib/dawos/measure
RATES=${RATES:-96000 48000 192000}
MARGINS=${MARGINS:-400 300 250 200 150 100 50}
LOAD=${LOAD:-120}
SECONDS_JUDGED=${JUDGED:-30}
WARM_UP=8
DEVICE_OPTIONS=${DEVICE_OPTIONS:-}

die() {
	echo "audio-margin: $*" >&2
	exit 1
}

field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

[ "$(id -u)" = 0 ] || die "must run as root"
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
dir="${BASE}/margin-${stamp}${label:+-${label}}"
mkdir -p "${dir}" || die "cannot create ${dir}"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "engine: $("${ENGINE}" --version 2> /dev/null)"
	echo "card: ${identifier} (${model})"
	echo "rates: ${RATES}"
	echo "margins_us: ${MARGINS}"
	echo "load_percent: ${LOAD}"
	echo "seconds: ${SECONDS_JUDGED} after ${WARM_UP} of warm-up"
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
# cycle: the least time left before the end of the period, the least the
# card still had to play at a wake-up, the longest compute phase.
slot0() {
	awk -v what="$2" '
		/^record, slot 0:/ { inside = 1; next }
		/^record, / { inside = 0 }
		inside && what == "margin" && $1 == "least_margin_us:" { print $2 }
		inside && what == "to_play" && $1 == "card_frames_at_the_wake_up:" { print $8 }
		inside && what == "compute" && $1 == "compute," { print $9 }
		inside && what == "cut" && $1 == "periods_gone_by:" { sub(";", "", $8); print $8 }' "$1" | head -n 1
}

# One run: $1 rate, $2 load, $3 margin.
run() {
	name="$1-load$2-margin$3"
	# shellcheck disable=SC2086  # device is several words
	"${ENGINE}" run --backend alsa ${device} --rate "$1" --load "$2" --margin-us "$3" \
		--seconds "$((SECONDS_JUDGED + WARM_UP))" --warm-up "${WARM_UP}" --require-rt \
		--tone 3 --tone-cycles 3 --listen 3 --count 5:5 \
		> "${dir}/${name}.txt" 2> "${dir}/${name}.err"
	printf '%-7s %5s %7s  %-28s %-34s %-9s %9s %9s %9s  %s\n' "$1" "$2" "$3" \
		"$(field "${dir}/${name}.txt" card_xruns | sed 's/ (.*//')" \
		"$(field "${dir}/${name}.txt" count_slips_on_input_5 | sed 's/;.*//')" \
		"$(slot0 "${dir}/${name}.txt" cut)" \
		"$(slot0 "${dir}/${name}.txt" compute)" \
		"$(slot0 "${dir}/${name}.txt" margin)" \
		"$(slot0 "${dir}/${name}.txt" to_play)" \
		"$(field "${dir}/${name}.txt" audio | cut -c1-60)"
}

{
	printf '%-7s %5s %7s  %-28s %-34s %-9s %9s %9s %9s  %s\n' "rate" "load" "margin" "xruns" "slips of the count" "cut" \
		"compute" "left_us" "to_play" "audio"
	for rate in ${RATES}; do
		# The interface needs some seconds after a change of rate, and
		# the streams of the runs that count must start afresh.
		# shellcheck disable=SC2086  # device is several words
		"${ENGINE}" run --backend alsa ${device} --rate "${rate}" --seconds 8 --threads 1 \
			--allow-xruns --require-rt > "${dir}/settle-${rate}.txt" 2>&1
		for margin in ${MARGINS}; do
			run "${rate}" "${LOAD}" "${margin}"
		done
		run "${rate}" 60 150
	done
} | tee "${dir}/summary.txt"

[ -n "${DEVICE_OPTIONS}" ] || "${SETUP}" apply --card "${card}" > "${dir}/routing-after.txt" 2>&1
{
	echo
	echo "columns: cut, the cycles of the thread that holds the card ended by the compute budget;"
	echo "  compute, its longest compute phase in microseconds; left_us, the least time left before"
	echo "  the end of the period (negative: after it); to_play, the least frames the card still had"
	echo "  to play at a wake-up."
	echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} | tee -a "${dir}/summary.txt"
