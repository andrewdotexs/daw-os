#!/bin/sh
# audio-diagnose.sh — where does the test tone of the engine go?
#
# Step P1.4: on the Scarlett 18i20 the engine ran, and the tone it played
# on output 3 was not found on input 3. This reads, without touching a
# cable, how far the tone gets:
#
#   1. the state of the router of the interface;
#   2. its level meters at rest;
#   3. for each of the first ten playback channels: the engine plays the
#      tone there for six seconds and listens to the first ten capture
#      channels; half way the level meters of the interface are read.
#      The meters say whether the interface receives the tone from the
#      computer and where it sends it; the inputs say where it comes
#      back, whichever jacks the cable is in;
#   4. the same tone on output 3 from another program (speaker-test), to
#      tell the engine from the rest.
#
# The meters are the control "Level Meter" (0 to 4095), and which meter
# is which depends on the interface: here they are numbered from 1 and
# only those that are not at zero are shown. On the Scarlett 18i20 of
# the second generation (driver scarlett2) there is one per destination
# of the router: the ten line outputs first, the eighteen capture
# channels last. On the fourth generation fcp-server puts the sources
# first, in the order of its map (analogue inputs 1-8, the microphone of
# the panel, S/PDIF, ADAT, the playback channels), then the
# destinations; only those the interface has a meter for (step P1.12).
#
# On the fourth generation capture channels 9 and 10 take playback
# channels 1 and 2 back inside the interface, in its known state: the
# tone on output 1 must be heard on input 9 whatever the cables.
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s < tools/audio-diagnose.sh > FILE
#
# About a minute and a half. A tone of 1 kHz comes out of every line
# output in turn: keep monitors and headphones down, and nothing that
# amplifies on the line outputs.

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
	amixer -c "${card}" cget iface=PCM,name='Level Meter' 2> /dev/null | sed -n 's/^  : values=//p'
}

# One line: the meters that are not at zero, as NUMBER=VALUE, numbered
# from 1.
show_meters() {
	meters | awk -F, -v label="$1" '{
		printf "%s", label
		shown = 0
		for (i = 1; i <= NF; i++) if ($i + 0 != 0) { printf " %d=%s", i, $i; shown++ }
		if (!shown) printf " all %d at zero", NF
		printf "\n"
	}'
}

# Channels of the first playback setting of the card.
playback_channels() {
	awk '$1 == "Playback:" { inside = 1 } $1 == "Capture:" { inside = 0 }
		inside && $1 == "Channels:" { print $2; exit }' "/proc/asound/card${card}/stream0" 2> /dev/null
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
echo "all of them: $(meters)"

echo
echo "== 3. the tone of the engine on each of the first ten outputs, six seconds each =="
echo "(levels in dB relative to full scale; the tone leaves at -12)"
for output in 1 2 3 4 5 6 7 8 9 10; do
	echo
	echo "-- tone on output ${output} --"
	"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --seconds 6 \
		--tone "${output}" --listen 1,2,3,4,5,6,7,8,9,10 --allow-xruns --require-rt \
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
speaker-test -D "hw:${identifier},0" -c "$(playback_channels)" -r 48000 -F S32_LE -t sine -f 1000 -s 3 \
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
