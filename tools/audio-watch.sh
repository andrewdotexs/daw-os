#!/bin/sh
# audio-watch.sh — what the kernel says about the two streams of the
# sound card, ten times a second, while the engine plays and listens.
#
# Step P1.4: on the Scarlett 18i20 the signal slips now and then (some
# samples lost or made up on the way round) with no xrun to tell. The
# engine reports every slip with its time; this adds, for the same run,
# what the driver saw around it:
#
#   time         seconds since the streams started
#   out_delay    frames between the last one written and the converters
#   out_room     frames of room in the output buffer
#   out_hw, out_appl   where the driver and the engine are in the output
#   in_ready     frames waiting to be read
#   in_hw        where the driver is in the input
#   rate         the rate the interface asks the computer to send at, as
#                the driver holds it (frames per USB microframe, 16.16)
#
# Reading these makes the kernel look at the streams between two
# periods: the wake-up delays of this run are not to be compared with
# those of a run left alone.
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s -- [RATE [PERIODS [SECONDS]]] < tools/audio-watch.sh > FILE
#
# Defaults: 48000, 2, 30. The tone (three cycles in a quantum) is on output 3 and is
# listened to on input 3, where the cable brings it.

set -u

PROG="audio-watch.sh"
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

rate=${1:-48000}
periods=${2:-2}
seconds=${3:-30}
for number in "${rate}" "${periods}" "${seconds}"; do
	case "${number}" in
	'' | *[!0-9]*) die "rate, periods and seconds are numbers" ;;
	esac
done
[ "$(id -u)" = 0 ] || die "must run as root"
card=${DAWOS_CARD:-}
if [ -z "${card}" ]; then
	card=$(first_usb_card) || die "no USB sound card: is the interface connected?"
fi
identifier=$(cat "/proc/asound/card${card}/id")
proc="/proc/asound/card${card}"
work=$(mktemp -d) || die "cannot create a work directory"
trap 'rm -rf "${work}"' EXIT

echo "== when and what =="
date -u '+read_utc: %Y-%m-%dT%H:%M:%SZ'
# shellcheck source=/dev/null
echo "image: $(. /etc/os-release && echo "${VERSION_ID:-unknown}")"
echo "card: ${card} (${identifier})"
echo "rate: ${rate}, periods: ${periods}, seconds: ${seconds}"

# The interface needs some seconds after a change of rate: not judged.
"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate "${rate}" --periods "${periods}" \
	--seconds 8 --tone 3 --tone-cycles 3 --listen 3 --allow-xruns --require-rt > "${work}/settle.txt" 2>&1
echo "settling run: xruns $(sed -n 's/^card_xruns: //p' "${work}/settle.txt")"

"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --rate "${rate}" --periods "${periods}" \
	--seconds "${seconds}" --tone 3 --tone-cycles 3 --listen 3 --each-second --require-rt \
	> "${work}/run.txt" 2> "${work}/run.err" &
engine_pid=$!

# Only a USB card has the file with the rate asked by the interface.
stream_file=
[ -r "${proc}/stream0" ] && stream_file="${proc}/stream0"

echo
echo "== the streams, ten times a second =="
echo "time out_delay out_room out_hw out_appl in_ready in_hw rate"
sleep 1
count=0
while kill -0 "${engine_pid}" 2> /dev/null && [ "${count}" -lt $((seconds * 12)) ]; do
	# shellcheck disable=SC2086  # stream_file is one word or none
	awk -F: '
		function value(text) { gsub(/[ \t]/, "", text); return text }
		FILENAME ~ /pcm0p/ {
			key = value($1)
			if (key == "trigger_time") start = value($2)
			if (key == "tstamp") now = value($2)
			if (key == "delay") out_delay = value($2)
			if (key == "avail") out_room = value($2)
			if (key == "hw_ptr") out_hw = value($2)
			if (key == "appl_ptr") out_appl = value($2)
		}
		FILENAME ~ /pcm0c/ {
			key = value($1)
			if (key == "avail") in_ready = value($2)
			if (key == "hw_ptr") in_hw = value($2)
		}
		FILENAME ~ /stream0/ {
			if ($0 ~ /^Playback/) playback = 1
			if ($0 ~ /^Capture/) playback = 0
			if (playback && $0 ~ /Momentary freq/ && rate == "") {
				rate = $0
				sub(/^.*\(/, "", rate)
				sub(/\).*$/, "", rate)
			}
		}
		END {
			if (now != "")
				printf "%.3f %s %s %s %s %s %s %s\n", now - start, out_delay, out_room, out_hw, out_appl, in_ready, in_hw, rate
		}' "${proc}/pcm0p/sub0/status" "${proc}/pcm0c/sub0/status" ${stream_file} 2> /dev/null
	sleep 0.1
	count=$((count + 1))
done
wait "${engine_pid}"
engine_status=$?
echo
echo "== the report of the engine (exit status ${engine_status}) =="
sed -n '/^card_capture/,$p' "${work}/run.txt"
[ -s "${work}/run.err" ] && sed 's/^/stderr: /' "${work}/run.err"
echo
echo "== the thread =="
sed -n '/^slot/,/^$/p' "${work}/run.txt"
