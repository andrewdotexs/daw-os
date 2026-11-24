#!/bin/sh
# card-firmware.sh — the firmware of a Focusrite interface of the
# Focusrite Control Protocol (the Scarlett 18i20 of the fourth
# generation): which version it runs, and its update (step P1.12).
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s -- status < tools/card-firmware.sh
#   ssh "root@${H}" sh -s -- update < tools/card-firmware.sh
#
# "status" only reads: the interface, the version of its firmware, and
# whether the image holds a newer one (package scarlett4-firmware).
#
# "update" writes the firmware of the image into the interface, with
# `fcp-tool update`. It is never done by the system on its own: only by
# this command, given by hand. What it takes care of:
#
# - it refuses while a program has the sound card open: the interface
#   restarts in the middle of the update, and more than once;
# - fcp-tool runs as a unit of systemd (dawos-card-firmware), not as a
#   child of the SSH session: if the connection drops, the update goes
#   on to its end. This script only waits for it and shows what it
#   wrote; run "status" to see how it ended if the connection dropped;
# - the serial number of the interface, which fcp-tool prints, is left
#   out of everything shown here.
#
# The update takes one or two minutes. If it fails half-way the
# interface is not lost: give the command again, and it resumes from the
# part that is missing (so the documentation of fcp-support says).
#
# To try the script itself where there is no interface (a virtual
# machine), DAWOS_FCP_TOOL names a program to run instead of fcp-tool and
# DAWOS_ASOUND a directory to read instead of /proc/asound.
#
# Exit status: 0 done, or nothing to do; 1 the update failed, or was
# refused; 2 usage.

set -u

PROG="card-firmware.sh"
UNIT=dawos-card-firmware
# Where the unit leaves the exit status of fcp-tool.
STATUS=/run/dawos-card-firmware.status
TOOL=${DAWOS_FCP_TOOL:-fcp-tool}
ASOUND=${DAWOS_ASOUND:-/proc/asound}
# Seconds to wait for the update, and then for the interface and its
# server to be there again.
UPDATE_LIMIT=420
BACK_LIMIT=60

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

# Lines without the serial number of the interface: `fcp-tool list` has
# a line for it, and every other command names the interface it chose
# with the serial number in brackets.
without_serial() {
	grep -v -i '^ *serial' | sed 's/^\(Selected device [^(]*\)(.*)$/\1(serial number left out)/'
}

# What the unit of the update wrote since the time $1 (as journalctl
# reads it). fcp-tool draws a progress bar over itself with carriage
# returns: those states are left out.
update_messages() {
	journalctl -b -u "${UNIT}" --since "$1" --no-pager -o cat 2> /dev/null | tr '\r' '\n' |
		grep -v -E '^\[.*\] *[0-9]+%$' | grep -v '^$' | without_serial
}

# Number of the first sound card that is a Focusrite USB device.
focusrite_card() {
	for directory in "${ASOUND}"/card[0-9]*; do
		[ -r "${directory}/usbid" ] || continue
		case "$(cat "${directory}/usbid")" in
		1235:*)
			echo "${directory##*/card}"
			return 0
			;;
		esac
	done
	return 1
}

# Is a stream of card $1 open? The status file of a stream that no
# program holds says "closed".
card_in_use() {
	for status in "${ASOUND}/card$1"/pcm*/sub*/status; do
		[ -r "${status}" ] || continue
		grep -q '^closed' "${status}" || return 0
	done
	return 1
}

do_status() {
	command -v "${TOOL}" > /dev/null 2>&1 || die "fcp-tool is not in this image"
	card=$(focusrite_card) || die "no Focusrite sound card: is the interface connected and switched on?"
	echo "read_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "card: ${card} ($(cat "${ASOUND}/card${card}/id"), usb_id $(cat "${ASOUND}/card${card}/usbid"))"
	echo "server: fcp-server@${card}.service: $(systemctl is-active "fcp-server@${card}.service" 2> /dev/null)"
	echo "firmware_files: $(find /usr/lib/firmware/scarlett4 -name '*.bin' 2> /dev/null | sed 's|.*/||' | tr '\n' ' ')"
	"${TOOL}" list 2>&1 | without_serial
	if systemctl --quiet is-active "${UNIT}" 2> /dev/null; then
		echo "update: running now"
	elif [ -r "${STATUS}" ]; then
		echo "update: the last one of this boot ended with status $(cat "${STATUS}"), and wrote:"
		update_messages "-1year" | tail -n 30 | sed 's/^/  /'
	fi
}

do_update() {
	[ "$(id -u)" = 0 ] || die "must run as root"
	command -v "${TOOL}" > /dev/null 2>&1 || die "fcp-tool is not in this image"
	card=$(focusrite_card) || die "no Focusrite sound card: is the interface connected and switched on?"
	systemctl --quiet is-active "${UNIT}" 2> /dev/null && die "an update is running: wait for it (status)"
	systemctl --quiet is-active "fcp-server@${card}.service" 2> /dev/null ||
		die "fcp-server@${card}.service is not running: without it nothing can be written to the interface"
	card_in_use "${card}" && die "a program has the sound card open: stop it first"
	for unit in dawos-audio-play dawos-audio-record dawos-audio-hours; do
		systemctl --quiet is-active "${unit}" 2> /dev/null && die "${unit} is running: stop it first"
	done

	echo "== before =="
	"${TOOL}" list 2>&1 | without_serial
	if "${TOOL}" list 2>&1 | grep -q -E 'up to date|newer than'; then
		echo "nothing to do: the interface does not run a firmware older than the one of the image"
		return 0
	fi
	"${TOOL}" list 2>&1 | grep -q 'update available' ||
		die "fcp-tool does not offer an update (see above): nothing was written"

	echo
	echo "== update (started $(date -u +%H:%M:%SZ); it goes on if this connection drops) =="
	systemctl reset-failed "${UNIT}" 2> /dev/null
	rm -f "${STATUS}"
	since=$(date '+%Y-%m-%d %H:%M:%S')
	# shellcheck disable=SC2016  # expanded by the shell of the unit
	systemd-run --quiet --collect --unit="${UNIT}" --description="Firmware update of the audio interface" \
		sh -c '"$2" update; echo "$?" > "$1"' update "${STATUS}" "${TOOL}" || die "could not start the update"
	waited=0
	while systemctl --quiet is-active "${UNIT}" 2> /dev/null; do
		[ "${waited}" -lt "${UPDATE_LIMIT}" ] || die "the update is still running after ${UPDATE_LIMIT} s: left alone; see it with status"
		sleep 5
		waited=$((waited + 5))
		echo "  ... ${waited} s"
	done
	update_messages "${since}"
	result=$(cat "${STATUS}" 2> /dev/null)
	echo "fcp_tool_exit_status: ${result:-unknown}"
	echo "seconds: ${waited}"

	echo
	echo "== after =="
	waited=0
	while :; do
		if card=$(focusrite_card) && systemctl --quiet is-active "fcp-server@${card}.service" 2> /dev/null &&
			"${TOOL}" list > /dev/null 2>&1; then
			break
		fi
		[ "${waited}" -lt "${BACK_LIMIT}" ] || die "the interface or its server did not come back in ${BACK_LIMIT} s"
		sleep 2
		waited=$((waited + 2))
	done
	"${TOOL}" list 2>&1 | without_serial
	if [ "${result:-1}" = 0 ] && "${TOOL}" list 2>&1 | grep -q 'up to date'; then
		echo "update: done"
		return 0
	fi
	echo "update: NOT done (give the command again: it resumes from the part that is missing)"
	return 1
}

case "${1:-}" in
status) do_status ;;
update) do_update ;;
*)
	echo "usage: ${PROG} status | update" >&2
	exit 2
	;;
esac
