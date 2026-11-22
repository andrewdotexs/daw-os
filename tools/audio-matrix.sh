#!/bin/sh
# audio-matrix.sh — do the slips of the signal depend on the buffers?
#
# Step P1.4: on the Scarlett 18i20 the signal slips by a few samples now
# and then, with no xrun. This runs the engine for a minute in several
# shapes (sample rate, frames per period, periods per buffer) and lists
# the slips of each: from the engine's own two short periods to buffers
# as long as those of an ordinary program.
#
# The tone (1125 Hz) is on output 3 and is listened to on input 3, where
# the cable brings it. The router of the interface is not touched.
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s -- [SECONDS] < tools/audio-matrix.sh > FILE
#
# Default: 60 seconds per shape, nine shapes: about eleven minutes.

set -u

PROG="audio-matrix.sh"
ENGINE=${DAWOS_ENGINE:-dawos-engine}

# Rate, frames per period, periods per buffer. Every period here holds
# a whole number of cycles of the tone.
SHAPES="192000:512:2 192000:512:3 192000:512:4 192000:512:8 192000:2048:2 192000:2048:4 48000:128:2 48000:128:4 48000:1024:4"

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

first_usb_card() {
	for directory in /proc/asound/card[0-9]*; do
		[ -r "${directory}/usbid" ] || continue
		echo "${directory##*/card}"
		return 0
	done
	return 1
}

field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

seconds=${1:-60}
case "${seconds}" in
'' | *[!0-9]*) die "seconds is a number" ;;
esac
[ "$(id -u)" = 0 ] || die "must run as root"
card=${DAWOS_CARD:-}
if [ -z "${card}" ]; then
	card=$(first_usb_card) || die "no USB sound card: is the interface connected?"
fi
identifier=$(cat "/proc/asound/card${card}/id")
work=$(mktemp -d) || die "cannot create a work directory"
trap 'rm -rf "${work}"' EXIT

echo "== when and what =="
date -u '+read_utc: %Y-%m-%dT%H:%M:%SZ'
# shellcheck source=/dev/null
echo "image: $(. /etc/os-release && echo "${VERSION_ID:-unknown}")"
echo "card: ${card} (${identifier})"
echo "seconds per shape: ${seconds}"
echo "lowlatency: $(cat /sys/module/snd_usb_audio/parameters/lowlatency 2> /dev/null)"

current_rate=
for shape in ${SHAPES}; do
	rate=${shape%%:*}
	rest=${shape#*:}
	quantum=${rest%%:*}
	periods=${rest#*:}
	if [ "${rate}" != "${current_rate}" ]; then
		# The interface needs some seconds after a change of rate.
		"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate "${rate}" --seconds 8 \
			--tone 3 --tone-hz 1125 --listen 3 --allow-xruns --require-rt > /dev/null 2>&1
		current_rate=${rate}
	fi
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate "${rate}" --quantum "${quantum}" \
		--periods "${periods}" --seconds "${seconds}" --tone 3 --tone-hz 1125 --listen 3 --each-second \
		--require-rt > "${work}/run.txt" 2> "${work}/run.err"
	echo
	echo "-- rate ${rate}, period ${quantum} frames, ${periods} periods ($((quantum * periods * 1000 / rate)) ms of buffer) --"
	echo "xruns: $(field "${work}/run.txt" card_xruns); slips: $(field "${work}/run.txt" slips_on_input_3); $(field "${work}/run.txt" card_cycles)"
	sed -n '/^slot/,/^$/p' "${work}/run.txt" | grep -v '^$'
	grep 'slip on input' "${work}/run.txt"
	awk '
		/^second +input/ { inside = 1; next }
		inside && $1 !~ /^[0-9]+$/ { inside = 0 }
		inside && $1 > 1 && $5 < 0.99995 { printf "  second %s: level %s, tone %s, share %s\n", $1, $3, $4, $5 }' "${work}/run.txt"
	grep '^card_xrun_' "${work}/run.txt"
	[ -s "${work}/run.err" ] && sed 's/^/stderr: /' "${work}/run.err"
done

echo
echo "== kernel messages about sound and USB during the runs =="
journalctl -k --since="-$((seconds * 10 + 120))s" --no-pager -o cat 2> /dev/null | grep -i -E 'usb|snd|alsa|scarlett|xhci' || echo "(none)"
