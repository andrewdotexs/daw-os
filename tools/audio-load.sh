#!/bin/sh
# audio-load.sh — a USB audio interface that plays and records without
# pause, as a load for the latency measurements (step P1.0, part 3b; open
# point A-41), and a reading of how the kernel sees the interface.
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s -- probe            < tools/audio-load.sh > FILE
#   ssh "root@${H}" sh -s -- start [OPTIONS]  < tools/audio-load.sh
#   ssh "root@${H}" sh -s -- status           < tools/audio-load.sh
#   ssh "root@${H}" sh -s -- stop             < tools/audio-load.sh
#   ssh "root@${H}" sh -s -- card [N]         < tools/audio-load.sh > FILE
#
# "card" reads what step P1.4 needs before the engine opens the interface:
# every mixer control with its value, and, for each sample rate, whether
# the driver accepts two periods of one quantum (2.667 ms) in mmap access,
# with the parameters it really sets. It plays silence and records for
# four seconds per rate: about half a minute in all. For an interface of
# the Focusrite Control Protocol (step P1.12) it also reads the state of
# fcp-server, without which there are no mixer controls to read, the
# version of the firmware, and whether the interface still shows the
# small disk it leaves the factory with.
#
# Options of start:
#   --card N        the sound card (default: the first USB one)
#   --rate HZ       sample rate (default 48000)
#   --period N      frames per period (default 64: 1.3 ms at 48 kHz)
#   --buffer N      frames of buffer (default: two periods)
#   --no-record     playback only
#   --no-play       capture only
#
# What it loads: the interrupts of the USB controller the interface is
# on, and the work the kernel does in them for every period of audio.
# That is what a session adds to a machine, and what the campaign of step
# P0.7 did not have. aplay plays silence from /dev/zero and arecord
# records into /dev/null, with the format and the number of channels the
# interface declares; both run on the CPUs of the "system" domain with a
# real-time priority, so that the load of a measurement on those CPUs
# does not starve them. They stand in for the engine, which does not
# exist yet: nothing here runs on a live CPU.
#
# "probe" only reads. The serial number of the interface is never
# printed.

set -u

PROG="audio-load.sh"
PLAY_UNIT=dawos-audio-play
RECORD_UNIT=dawos-audio-record
STATE=/run/dawos-audio-load
TOPOLOGY=/run/dawos/topology.json

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

read_file() {
	[ -r "$1" ] && cat "$1" 2> /dev/null
}

# CPU list of a domain of the boot profile, from the report of dawos-tune.
domain() {
	sed -n "s/^ *\"$1\": \"\\([0-9,-]*\\)\".*/\\1/p" "${TOPOLOGY}" 2> /dev/null | head -n 1
}

# Number of the first sound card that is a USB device.
first_usb_card() {
	for directory in /proc/asound/card[0-9]*; do
		[ -r "${directory}/usbid" ] || continue
		echo "${directory##*/card}"
		return 0
	done
	return 1
}

# "FORMAT CHANNELS" of the first setting of a direction (Playback or
# Capture) in the stream description of card $1.
stream_shape() {
	awk -v direction="$2:" '
		$1 == "Playback:" || $1 == "Capture:" { inside = ($1 == direction) }
		inside && $1 == "Format:" && format == "" { format = $2 }
		inside && $1 == "Channels:" && channels == "" { channels = $2 }
		END { if (format != "" && channels != "") print format, channels }' \
		"/proc/asound/card$1/stream0" 2> /dev/null
}

# PCI address of the USB controller card $1 hangs from, and the path of
# the USB device below it.
usb_place() {
	path=$(readlink -f "/sys/class/sound/card$1/device" 2> /dev/null) || return 1
	controller=$(echo "${path}" | sed -n 's|.*/\([0-9a-f]\{4\}:[0-9a-f]\{2\}:[0-9a-f]\{2\}\.[0-9a-f]\)/usb[0-9]*/.*|\1|p')
	port=$(echo "${path}" | sed -n 's|.*/usb[0-9]*/\(.*\)/[^/]*$|\1|p')
	usb_device=$(echo "${path}" | sed 's|/[^/]*$||')
	echo "${controller:-unknown} ${port:-unknown} ${usb_device}"
}

# Interrupt numbers of the PCI device $1, one per line.
controller_irqs() {
	if [ -d "/sys/bus/pci/devices/$1/msi_irqs" ]; then
		ls "/sys/bus/pci/devices/$1/msi_irqs"
	else
		read_file "/sys/bus/pci/devices/$1/irq"
	fi
}

# Sum of the counts of interrupt $1 on the CPUs of list $2 ("4-7"), or on
# all CPUs when $2 is empty.
irq_count() {
	awk -v irq="$1:" -v list="$2" '
		BEGIN {
			n = split(list, parts, ",")
			for (i = 1; i <= n; i++) {
				m = split(parts[i], ends, "-")
				for (cpu = ends[1]; cpu <= (m > 1 ? ends[2] : ends[1]); cpu++) wanted[cpu] = 1
			}
		}
		NR == 1 { cpus = NF; next }
		$1 == irq {
			for (i = 2; i <= cpus + 1; i++) if (list == "" || (i - 2) in wanted) sum += $i
			print sum + 0
			found = 1
		}
		END { if (!found) print 0 }' /proc/interrupts
}

# Largest number of channels of a direction (Playback or Capture) of card
# $1 among the settings that offer the rate $3.
channels_at() {
	awk -v direction="$2:" -v rate="$3" '
		$1 == "Playback:" || $1 == "Capture:" { inside = ($1 == direction) }
		inside && $1 == "Channels:" { channels = $2 + 0 }
		inside && $1 == "Rates:" {
			for (i = 2; i <= NF; i++) {
				value = $i
				sub(/,/, "", value)
				if (value == rate && channels > best) best = channels
			}
		}
		END { if (best > 0) print best }' "/proc/asound/card$1/stream0" 2> /dev/null
}

choose_card() {
	card=${1:-}
	if [ -z "${card}" ]; then
		card=$(first_usb_card) || die "no USB sound card: is the interface connected? (see /proc/asound/cards)"
	fi
	[ -d "/proc/asound/card${card}" ] || die "there is no sound card ${card}"
}

do_probe() {
	echo "== when and what =="
	date -u '+read_utc: %Y-%m-%dT%H:%M:%SZ'
	echo "uptime_s: $(cut -d. -f1 /proc/uptime)"
	echo "kernel: $(uname -r)"
	echo "live_cpus: $(domain live)"
	echo "io_cpus: $(domain io)"
	echo
	echo "== sound cards =="
	cat /proc/asound/cards 2> /dev/null
	echo
	card=$(first_usb_card) || {
		echo "no USB sound card"
		return 0
	}
	echo "== card ${card} =="
	echo "usb_id: $(read_file "/proc/asound/card${card}/usbid")"
	echo "playback: $(stream_shape "${card}" Playback)"
	echo "capture: $(stream_shape "${card}" Capture)"
	place=$(usb_place "${card}")
	controller=${place%% *}
	rest=${place#* }
	echo "usb_controller: ${controller}"
	echo "usb_port: ${rest%% *}"
	usb_device=${rest#* }
	echo "usb_speed_mbps: $(read_file "${usb_device}/speed")"
	echo "usb_version: $(read_file "${usb_device}/version" | tr -d ' ')"
	echo "usb_power_ma: $(read_file "${usb_device}/bMaxPower")"
	echo "controller_driver: $(basename "$(readlink "/sys/bus/pci/devices/${controller}/driver" 2> /dev/null)" 2> /dev/null)"
	for irq in $(controller_irqs "${controller}"); do
		# A controller has many interrupt lines and uses few: only the
		# ones that fired are worth a line.
		[ "$(irq_count "${irq}" "")" -gt 0 ] || continue
		echo "controller_irq: ${irq} on CPUs $(read_file "/proc/irq/${irq}/effective_affinity_list") (allowed: $(read_file "/proc/irq/${irq}/smp_affinity_list")), count $(irq_count "${irq}" ""), on the live CPUs $(irq_count "${irq}" "$(domain live)")"
	done
	echo "other_devices_on_the_controller: $(find "/sys/bus/pci/devices/${controller}/" -maxdepth 3 -name 'product' 2> /dev/null | while read -r file; do tr '\n' ';' < "${file}"; done)"
	echo
	echo "== driver =="
	for file in /sys/module/snd_usb_audio/parameters/lowlatency /sys/module/snd_usb_audio/parameters/implicit_fb; do
		[ -r "${file}" ] && echo "${file##*/}: $(tr '\n' ' ' < "${file}")"
	done
	echo
	echo "== streams as the driver describes them =="
	head -n 80 "/proc/asound/card${card}/stream0" 2> /dev/null
	echo
	echo "== what playback accepts (aplay --dump-hw-params) =="
	shape=$(stream_shape "${card}" Playback)
	if [ -n "${shape}" ]; then
		aplay -D "hw:${card},0" -t raw -f "${shape% *}" -c "${shape#* }" -r 48000 -d 1 --dump-hw-params /dev/zero 2>&1 | head -n 30
	fi
}

do_start() {
	card=
	rate=48000
	period=64
	buffer=
	play=yes
	record=yes
	while [ $# -gt 0 ]; do
		case "$1" in
		--card)
			card=${2:-}
			shift 2
			;;
		--rate)
			rate=${2:-}
			shift 2
			;;
		--period)
			period=${2:-}
			shift 2
			;;
		--buffer)
			buffer=${2:-}
			shift 2
			;;
		--no-record)
			record=no
			shift
			;;
		--no-play)
			play=no
			shift
			;;
		*) die "unknown option '$1'" ;;
		esac
	done
	for number in "${rate}" "${period}" "${buffer:-0}"; do
		case "${number}" in
		'' | *[!0-9]*) die "rate, period and buffer are numbers" ;;
		esac
	done
	buffer=${buffer:-$((period * 2))}
	[ "$(id -u)" = 0 ] || die "must run as root"
	choose_card "${card}"
	if systemctl --quiet is-active "${PLAY_UNIT}" 2> /dev/null || systemctl --quiet is-active "${RECORD_UNIT}" 2> /dev/null; then
		die "already running: stop it first"
	fi
	cpus=$(domain system)
	[ -n "${cpus}" ] || die "no 'system' domain in ${TOPOLOGY}: is the boot profile applied?"

	place=$(usb_place "${card}")
	controller=${place%% *}
	mkdir -p "${STATE}"
	{
		echo "card=${card}"
		echo "controller=${controller}"
		echo "started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
		echo "requested=rate ${rate}, period ${period}, buffer ${buffer}"
		for irq in $(controller_irqs "${controller}"); do
			echo "irq_start=${irq} $(irq_count "${irq}" "") $(irq_count "${irq}" "$(domain live)")"
		done
	} > "${STATE}/run.txt"

	if [ "${play}" = yes ]; then
		shape=$(stream_shape "${card}" Playback)
		[ -n "${shape}" ] || die "card ${card} declares no playback stream"
		systemd-run --quiet --collect --unit="${PLAY_UNIT}" \
			chrt -f 60 taskset -c "${cpus}" \
			aplay -D "hw:${card},0" -t raw -f "${shape% *}" -c "${shape#* }" -r "${rate}" \
			--period-size="${period}" --buffer-size="${buffer}" /dev/zero ||
			die "could not start playback"
	fi
	if [ "${record}" = yes ]; then
		shape=$(stream_shape "${card}" Capture)
		if [ -z "${shape}" ]; then
			echo "${PROG}: card ${card} declares no capture stream: playback only"
		else
			systemd-run --quiet --collect --unit="${RECORD_UNIT}" \
				chrt -f 60 taskset -c "${cpus}" \
				arecord -D "hw:${card},0" -t raw -f "${shape% *}" -c "${shape#* }" -r "${rate}" \
				--period-size="${period}" --buffer-size="${buffer}" /dev/null ||
				die "could not start capture"
		fi
	fi
	sleep 2
	do_status
}

# One trial of "card": card $1 at rate $2 with a quantum of $3 frames, on
# the CPUs $4. Playback and capture run together for four seconds, with
# two periods of one quantum and mmap access, as the engine will ask.
card_trial() {
	play_channels=$(channels_at "$1" Playback "$2")
	record_channels=$(channels_at "$1" Capture "$2")
	echo
	echo "-- rate $2, quantum $3, buffer $(($3 * 2)): playback ${play_channels:-no} channels, capture ${record_channels:-no} channels --"
	if [ -z "${play_channels}" ] || [ -z "${record_channels}" ]; then
		echo "result: the interface does not offer this rate in both directions"
		return 0
	fi
	shape=$(stream_shape "$1" Playback)
	format=${shape% *}
	chrt -f 60 taskset -c "$4" \
		arecord -M -v -D "hw:$1,0" -t raw -f "${format}" -c "${record_channels}" -r "$2" \
		--period-size="$3" --buffer-size="$(($3 * 2))" -d 4 /dev/null > "${STATE}/card-record.txt" 2>&1 &
	record_pid=$!
	chrt -f 60 taskset -c "$4" \
		aplay -M -v -D "hw:$1,0" -t raw -f "${format}" -c "${play_channels}" -r "$2" \
		--period-size="$3" --buffer-size="$(($3 * 2))" -d 4 /dev/zero > "${STATE}/card-play.txt" 2>&1 &
	play_pid=$!
	sleep 2
	echo "while running, as the kernel reports it:"
	for stream in pcm0p pcm0c; do
		for file in hw_params sw_params; do
			echo "  ${stream} ${file}: $(tr '\n' ' ' < "/proc/asound/card$1/${stream}/sub0/${file}" 2> /dev/null)"
		done
	done
	awk '
		$1 == "Playback:" || $1 == "Capture:" { name = $1 }
		/Status:/ { running = /Running/ }
		/^  Interface / { running = 0 }
		running { print "  " name $0 }' "/proc/asound/card$1/stream0" 2> /dev/null
	wait "${play_pid}"
	play_status=$?
	wait "${record_pid}"
	record_status=$?
	echo "playback: exit status ${play_status}, underruns reported $(grep -c -i 'underrun' "${STATE}/card-play.txt")"
	echo "capture: exit status ${record_status}, overruns reported $(grep -c -i 'overrun' "${STATE}/card-record.txt")"
	echo "playback, as aplay reports it:"
	sed 's/^/  /' "${STATE}/card-play.txt"
	echo "capture, as arecord reports it:"
	sed 's/^/  /' "${STATE}/card-record.txt"
	rm -f "${STATE}/card-play.txt" "${STATE}/card-record.txt"
	# The interface needs a moment to change its clock.
	sleep 1
}

# What there is to know of fcp-server for card $1: the kernel driver of
# the Focusrite Control Protocol only carries messages, and it is this
# server that reads the router, the mixer and the preamplifiers of the
# interface and shows them as mixer controls. udev starts one for every
# card of Focusrite; in front of a card of another driver it ends at
# once, and that is not a failure. fcp-tool prints the serial number of
# the interface, and the server may: those lines are left out.
fcp_report() {
	unit="fcp-server@$1.service"
	echo "hwdep: $(awk -v card="$1" '{ split($1, place, "-") } place[1] + 0 == card' /proc/asound/hwdep 2> /dev/null | tr '\n' ';')"
	if ! command -v fcp-server > /dev/null 2>&1; then
		echo "fcp_server: not in this image"
		return 0
	fi
	echo "fcp_server: $(fcp-server --version 2>&1 | head -n 1)"
	echo "unit: ${unit}: $(systemctl is-active "${unit}" 2> /dev/null), $(systemctl show -p Result -p NRestarts -p ExecMainStatus "${unit}" 2> /dev/null | tr '\n' ' ')"
	echo "socket_directory: $(find /run/fcp-server -maxdepth 1 2> /dev/null | tr '\n' ' ')"
	echo "firmware_files: $(find /usr/lib/firmware/scarlett4 -name '*.bin' 2> /dev/null | sed 's|.*/||' | tr '\n' ' ')"
	echo "-- fcp-tool list --"
	fcp-tool list 2>&1 | grep -v -i 'serial'
	echo "-- fcp-tool list-all --"
	fcp-tool list-all 2>&1 | grep -v -i 'serial'
	echo "-- messages of the unit since the boot (the last 40) --"
	journalctl -b -u "${unit}" --no-pager -o cat 2> /dev/null | grep -v -i 'serial' | tail -n 40
}

# The disks that are part of the USB device of card $1. An interface of
# Focusrite leaves the factory showing a small read-only disk with a link
# to the page where it is registered, and stays so until its firmware is
# updated or it is switched on with the 48V button held down.
card_disks() {
	place=$(usb_place "$1") || return 0
	usb_device=${place##* }
	found=no
	for block in /sys/block/sd*; do
		[ -d "${block}" ] || continue
		case "$(readlink -f "${block}")" in
		"${usb_device}"/*) ;;
		*) continue ;;
		esac
		found=yes
		echo "${block##*/}: $(read_file "${block}/device/vendor" | tr -s ' ') $(read_file "${block}/device/model" | tr -s ' '), $(read_file "${block}/size") sectors, read-only $(read_file "${block}/ro")"
	done
	[ "${found}" = yes ] || echo "(none)"
}

do_card() {
	[ "$(id -u)" = 0 ] || die "must run as root"
	choose_card "${1:-}"
	if systemctl --quiet is-active "${PLAY_UNIT}" 2> /dev/null || systemctl --quiet is-active "${RECORD_UNIT}" 2> /dev/null; then
		die "the audio load is running: stop it first"
	fi
	cpus=$(domain system)
	[ -n "${cpus}" ] || die "no 'system' domain in ${TOPOLOGY}: is the boot profile applied?"
	mkdir -p "${STATE}"

	echo "== when and what =="
	date -u '+read_utc: %Y-%m-%dT%H:%M:%SZ'
	echo "uptime_s: $(cut -d. -f1 /proc/uptime)"
	echo "kernel: $(uname -r)"
	echo "alsa_utils: $(aplay --version 2>&1 | head -n 1)"
	echo "alsa_lib: $(find /usr/lib -maxdepth 1 -name 'libasound.so.*' -type f 2> /dev/null | sed 's|.*/||' | tr '\n' ' ')"
	echo "card: ${card} ($(read_file "/proc/asound/card${card}/id"), usb_id $(read_file "/proc/asound/card${card}/usbid"))"
	echo "trials_on_cpus: ${cpus} (real-time priority 60)"

	echo
	echo "== the server of the interface (fcp-server) =="
	fcp_report "${card}"

	echo
	echo "== disks the interface itself shows =="
	card_disks "${card}"

	echo
	echo "== streams as the driver describes them =="
	cat "/proc/asound/card${card}/stream0" 2> /dev/null

	echo
	echo "== mixer controls, with their values (amixer contents) =="
	# The name an interface of Focusrite gives itself (control "Name",
	# a string of bytes) ends with digits of its serial number: its
	# value is left out.
	amixer -c "${card}" contents 2>&1 | awk '
		/^numid=/ { name_control = ($0 ~ /,name=\047Name\047$/) }
		name_control && /^  : values=/ {
			print "  : values=(left out: the name of the interface ends with digits of its serial number)"
			next
		}
		{ print }'

	echo
	echo "== trials: two periods of one quantum, mmap access, four seconds each =="
	for pair in 44100:128 48000:128 88200:256 96000:256 176400:512 192000:512; do
		card_trial "${card}" "${pair%:*}" "${pair#*:}" "${cpus}"
	done

	echo
	echo "== kernel messages about sound and USB in the last minute =="
	# When the interface was plugged in during that minute, the kernel
	# has written its serial number: that line is left out.
	journalctl -k --since=-1min --no-pager -o cat 2> /dev/null | grep -i -E 'usb|snd|alsa|scarlett|xhci' |
		grep -v -i 'serial' || echo "(none)"
}

# The lines of one stream that say how it runs.
stream_status() {
	directory=/proc/asound/card$1/pcm0$2/sub0
	echo "  state: $(sed -n 's/^state: //p' "${directory}/status" 2> /dev/null)$(grep -q closed "${directory}/status" 2> /dev/null && echo closed)"
	sed -n 's/^\(format\|channels\|rate\|period_size\|buffer_size\): /  \1: /p' "${directory}/hw_params" 2> /dev/null | tr '\n' ';'
	echo
}

do_status() {
	[ -r "${STATE}/run.txt" ] || die "nothing was started (${STATE}/run.txt)"
	card=$(sed -n 's/^card=//p' "${STATE}/run.txt")
	controller=$(sed -n 's/^controller=//p' "${STATE}/run.txt")
	sed -n 's/^started_utc=/started_utc: /p; s/^requested=/requested: /p' "${STATE}/run.txt"
	echo "now_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	for unit in "${PLAY_UNIT}" "${RECORD_UNIT}"; do
		echo "${unit}: $(systemctl is-active "${unit}" 2> /dev/null)"
	done
	echo "playback:"
	stream_status "${card}" p
	echo "capture:"
	stream_status "${card}" c
	# aplay and arecord write a line for every break in the stream.
	echo "breaks_in_playback: $(journalctl -b -u "${PLAY_UNIT}" --no-pager -o cat 2> /dev/null | grep -c -i -E 'underrun|xrun')"
	echo "breaks_in_capture: $(journalctl -b -u "${RECORD_UNIT}" --no-pager -o cat 2> /dev/null | grep -c -i -E 'overrun|xrun')"
	sed -n 's/^irq_start=//p' "${STATE}/run.txt" | while read -r irq before before_live; do
		now=$(irq_count "${irq}" "")
		[ "${now}" -gt 0 ] || continue
		now_live=$(irq_count "${irq}" "$(domain live)")
		echo "controller_irq ${irq}: $((now - before)) interrupts since the start, $((now_live - before_live)) of them on the live CPUs; now on CPUs $(read_file "/proc/irq/${irq}/effective_affinity_list")"
	done
	echo "last_messages:"
	journalctl -b -u "${PLAY_UNIT}" -u "${RECORD_UNIT}" --no-pager -o cat -n 6 2> /dev/null | sed 's/^/  /'
}

do_stop() {
	[ "$(id -u)" = 0 ] || die "must run as root"
	if [ -r "${STATE}/run.txt" ]; then
		do_status
	fi
	systemctl stop "${PLAY_UNIT}" "${RECORD_UNIT}" 2> /dev/null
	echo "stopped"
}

case "${1:-}" in
probe) do_probe ;;
start)
	shift
	do_start "$@"
	;;
status) do_status ;;
stop) do_stop ;;
card) do_card "${2:-}" ;;
*)
	echo "usage: ${PROG} probe | start [--card N] [--rate HZ] [--period N] [--buffer N] [--no-record] [--no-play] | status | stop | card [N]" >&2
	exit 1
	;;
esac
