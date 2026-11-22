#!/bin/sh
# audio-diagnose.sh — where does the test tone of the engine go?
#
# Step P1.4: on the Scarlett 18i20 the engine ran, and the tone it played
# on output 3 was not found on input 3. This reads, without touching a
# cable, how far the tone gets:
#
#   1. the state of the router of the interface;
#   2. its level meters at rest;
#   3. for each of the ten line outputs: the engine plays the tone there
#      for six seconds and listens to the eight analogue inputs; half way
#      the level meters of the interface are read. The meters say whether
#      the interface receives the tone from the computer; the inputs say
#      where it comes back, whichever jacks the cable is in;
#   4. the same tone on output 3 from another program (speaker-test), to
#      tell the engine from the rest.
#
# The meters are those of the driver (control "Level Meter", 0 to 4095):
# one per destination of the router, the ten line outputs first, the
# eighteen capture channels last.
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s < tools/audio-diagnose.sh > FILE
#
# About a minute and a half. A tone of 1 kHz comes out of every line
# output in turn: keep monitors and headphones down.

set -u

PROG="audio-diagnose.sh"
ENGINE=${DAWOS_ENGINE:-dawos-engine}

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

# The level meters, as the driver gives them: numbers separated by
# commas.
meters() {
	amixer -c "${card}" cget name='Level Meter' 2> /dev/null | sed -n 's/^  : values=//p'
}

# One line: the meters of the ten line outputs and of the capture
# channels 1 to 8 (fields 39 to 46: after 10 line outputs, 2 S/PDIF
# outputs, 8 ADAT outputs and 18 inputs of the mixer).
show_meters() {
	meters | awk -F, -v label="$1" '{
		printf "%s line outputs 1-10:", label
		for (i = 1; i <= 10; i++) printf " %s", $i
		printf " | capture channels 1-8:"
		for (i = 39; i <= 46; i++) printf " %s", $i
		printf "\n"
	}'
}

# The inputs the engine listened to, from its report $1: one line each.
show_heard() {
	awk '
		/^input +seconds/ { inside = 1; next }
		inside && $1 !~ /^[0-9]+$/ { inside = 0 }
		inside { printf "  input %s: seconds %s, peak %s, level %s to %s, tone %s to %s, share at least %s\n", $1, $2, $3, $4, $5, $6, $7, $8 }' "$1"
}

[ "$(id -u)" = 0 ] || die "must run as root"
card=${1:-}
if [ -z "${card}" ]; then
	card=$(first_usb_card) || die "no USB sound card: is the interface connected?"
fi
[ -d "/proc/asound/card${card}" ] || die "there is no sound card ${card}"
identifier=$(cat "/proc/asound/card${card}/id")
work=$(mktemp -d) || die "cannot create a work directory"
trap 'rm -rf "${work}"' EXIT

echo "== when and what =="
date -u '+read_utc: %Y-%m-%dT%H:%M:%SZ'
echo "uptime_s: $(cut -d. -f1 /proc/uptime)"
# shellcheck source=/dev/null
echo "image: $(. /etc/os-release && echo "${VERSION_ID:-unknown}")"
echo "card: ${card} (${identifier}, usb_id $(cat "/proc/asound/card${card}/usbid" 2> /dev/null))"
echo "engine: $("${ENGINE}" --version 2> /dev/null)"

echo
echo "== 1. the router of the interface =="
dawos-card-setup check --card "${card}" 2>&1

echo
echo "== 2. the level meters at rest (0 to 4095) =="
show_meters "at rest:"
echo "all 56: $(meters)"

echo
echo "== 3. the tone of the engine on each line output, six seconds each =="
echo "(levels in dB relative to full scale; the tone leaves at -12)"
for output in 1 2 3 4 5 6 7 8 9 10; do
	echo
	echo "-- tone on output ${output} --"
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --seconds 6 \
		--tone "${output}" --listen 1,2,3,4,5,6,7,8 --allow-xruns --require-rt \
		> "${work}/run.txt" 2> "${work}/run.err" &
	engine_pid=$!
	sleep 4
	show_meters "  meters while it plays:"
	wait "${engine_pid}"
	echo "  engine: exit status $?, xruns $(sed -n 's/^card_xruns: //p' "${work}/run.txt"), linked $(sed -n 's/^card_linked: //p' "${work}/run.txt"), $(sed -n 's/^card_cycles: //p' "${work}/run.txt")"
	show_heard "${work}/run.txt"
	[ -s "${work}/run.err" ] && sed 's/^/  stderr: /' "${work}/run.err"
	if [ "${output}" = 3 ]; then
		cp "${work}/run.txt" "${work}/output-3.txt"
	fi
done

echo
echo "== 4. the same tone on output 3 from speaker-test =="
speaker-test -D "hw:${identifier},0" -c 20 -r 48000 -F S32_LE -t sine -f 1000 -s 3 \
	> "${work}/speaker.txt" 2>&1 &
speaker_pid=$!
sleep 3
show_meters "  meters while it plays:"
kill "${speaker_pid}" 2> /dev/null
wait "${speaker_pid}" 2> /dev/null
echo "  speaker-test said:"
grep -v '^$' "${work}/speaker.txt" | head -n 12 | sed 's/^/    /'

echo
echo "== the whole report of the engine with the tone on output 3 =="
cat "${work}/output-3.txt" 2> /dev/null

echo
echo "== kernel messages about sound and USB in the last three minutes =="
journalctl -k --since=-3min --no-pager -o cat 2> /dev/null | grep -i -E 'usb|snd|alsa|scarlett|xhci' || echo "(none)"
