#!/bin/sh
# usb-look.sh — what goes on the USB bus between the driver and the
# sound card while the engine runs? (step P1.8b.)
#
# Meant to run on the machine under test, from the development PC:
#
#   ssh "root@${H}" sh -s -- [LABEL] < tools/usb-look.sh
#
# Step P1.8a measured the way round with a period of the driver shorter
# than the quantum of the engine, and found three things it could not
# explain: 3 ms under the buffers that are still there, a way round that
# changes by half a millisecond with the start, and a stream that loses
# one packet in three when a transfer is one packet long. All three are
# decided on the bus, where the engine does not look.
#
# The kernel shows every transfer of a bus as it is handed to the
# controller ("S") and as it comes back ("C"), with the time of each
# (usbmon). For each rate of RATES and each number of SLICES this script
# reads the bus from before the engine starts its streams until three
# seconds later, and says, for the input and for the output of the card:
#
#   - how many transfers a second, and of how many packets;
#   - how long a transfer stays with the controller, from "S" to "C":
#     for the output that is how far ahead of the wire the driver hands
#     the samples over, plus the length of the transfer;
#   - how many output transfers are waiting when one more is handed
#     over;
#   - by how many microframes the transfers follow each other, as the
#     controller numbers them: a hole is a packet that did not go;
#   - how the streams start: when the first input comes back, when the
#     first output is handed over, when it first carries samples that
#     are not silence.
#
# The engine runs with one live thread and the count on output 5, which
# comes back on input 5 inside the interface: its own report of the same
# run (way round, frames ready and to play at a wake-up) is kept beside
# what the bus says. Reading the bus takes time from the driver: these
# runs measure the bus, not the latency.
#
# Also read once: what the controller says it can do (its capability
# registers), and what the kernel logged meanwhile.
#
# To try the script where there is no interface the engine can open (a
# virtual machine with an emulated USB card), TRY_COMMAND replaces the
# engine with the command it holds, run once for each case.
#
# Results go to /var/lib/dawos/measure/usb-<time>[-LABEL].

set -u

label=${1:-}
ENGINE=${DAWOS_ENGINE:-dawos-engine}
SETUP=${DAWOS_CARD_SETUP:-dawos-card-setup}
CARDS_DIR=${DAWOS_CARDS_DIR:-/usr/share/dawos/cards}
BASE=/var/lib/dawos/measure
RATES=${RATES:-48000}
SLICES=${SLICES:-1 2 4 8}
TRY_COMMAND=${TRY_COMMAND:-}
# Seconds the bus is read for, from just before the engine is started.
READ_SECONDS=3

die() {
	echo "usb-look: $*" >&2
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
where=$(cat "/proc/asound/card${card}/usbbus")
bus=$(echo "${where%%/*}" | sed 's/^0*//')
device=${where##*/}
[ -n "${bus}" ] || die "the kernel does not say which bus the card is on"

[ -d /sys/kernel/debug/usb ] || mount -t debugfs debugfs /sys/kernel/debug 2> /dev/null
monitor="/sys/kernel/debug/usb/usbmon/${bus}u"
[ -r "${monitor}" ] || die "this kernel does not show bus ${bus} (${monitor} is not there)"

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/usb-${stamp}${label:+-${label}}"
mkdir -p "${dir}" || die "cannot create ${dir}"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "engine: $("${ENGINE}" --version 2> /dev/null)"
	echo "card: ${identifier} (${model})"
	echo "bus: ${bus}, device ${device}"
	echo "rates: ${RATES}"
	echo "slices: ${SLICES}"
	echo "command: ${TRY_COMMAND:-the engine}"
} > "${dir}/meta.txt"
echo "directory: ${dir}"
: > "${dir}/summary.txt"

say() {
	echo "$*" | tee -a "${dir}/summary.txt"
}

# ---- The controller of the bus, and what it says it can do. The
# registers are in debugfs, under the name of the controller on its own
# bus (PCI).
controller=$(readlink -f "/sys/bus/usb/devices/usb${bus}" 2> /dev/null)
controller=${controller%/usb"${bus}"}
controller=${controller##*/}
registers="/sys/kernel/debug/usb/xhci/${controller}/reg-cap"
say "-- the controller of bus ${bus}: ${controller:-unknown} --"
if [ -r "${registers}" ]; then
	cp "${registers}" "${dir}/controller-registers.txt"
	structural=$(sed -n 's/^HCSPARAMS2 *= *//p' "${registers}")
	capability=$(sed -n 's/^HCCPARAMS1 *= *//p' "${registers}")
	say "  HCSPARAMS2 ${structural:-not there}, HCCPARAMS1 ${capability:-not there}"
	if [ -n "${structural}" ] && [ -n "${capability}" ]; then
		# The time the controller wants a transfer before its turn (IST):
		# bits 0 to 2, in frames of 1 ms if bit 3 is set, in microframes
		# otherwise. Contiguous frame numbers (CFC): bit 11.
		ist=$((structural & 7))
		if [ $((structural & 8)) -ne 0 ]; then
			say "  the controller wants a transfer ${ist} frames (of 1 ms) before its turn"
		else
			say "  the controller wants a transfer ${ist} microframes (of 125 us) before its turn"
		fi
		if [ $((capability & 2048)) -ne 0 ]; then
			say "  it keeps the frame numbers of a stream contiguous (CFC): yes"
		else
			say "  it keeps the frame numbers of a stream contiguous (CFC): no, every transfer goes as soon as it can"
		fi
	fi
else
	say "  its registers are not shown (${registers})"
fi
moderation="/sys/kernel/debug/usb/xhci/${controller}/reg-runtime"
[ -r "${moderation}" ] && cp "${moderation}" "${dir}/controller-runtime.txt"

# ---- The router of the interface: the known state, then the loop
# inside, for the count.
loop="${CARDS_DIR}/${model}.test-loop.conf"
routed=no
if [ -z "${TRY_COMMAND}" ] && [ -r "${CARDS_DIR}/${model}.conf" ] && [ -r "${loop}" ]; then
	{
		"${SETUP}" apply --card "${card}"
		"${SETUP}" apply --card "${card}" "${loop}"
	} > "${dir}/routing-before.txt" 2>&1
	routed=yes
fi

kernel_lines_before=$(dmesg | wc -l)

# ---- What a file of the bus says. $1: the file; $2: the device on the
# bus, as ":NNN:".
#
# A line of the bus, for a transfer of sound (isochronous):
#   tag time S|C Zi|Zo:bus:device:endpoint status:interval:start[:errors] packets
#   then at most five packets as status:offset:length, the length of
#   the whole, and "=" with the first 32 bytes
# "start" is the microframe the controller gave the transfer.
read_bus() {
	awk -v device="$2" -v window=1000000 '
		# The keys of a table as numbers in order, in list[1..n].
		function in_order(table, list,    key, n, i, j, held) {
			n = 0
			for (key in table) list[++n] = key
			for (i = 2; i <= n; i++) {
				held = list[i]
				for (j = i - 1; j >= 1 && list[j] + 0 > held + 0; j--) list[j + 1] = list[j]
				list[j + 1] = held
			}
			return n
		}
		# The values of a table of counts that were seen most, at most
		# four of them, as "value (times)".
		function most_seen(table, prefix,    key, text, round, best, done) {
			text = ""
			for (round = 1; round <= 4; round++) {
				best = ""
				for (key in table) {
					if (index(key, prefix) != 1 || (key in done)) continue
					if (best == "" || table[key] > table[best]) best = key
				}
				if (best == "") break
				done[best] = 1
				text = text (text == "" ? "" : ", ") substr(best, length(prefix) + 1) " (" table[best] ")"
			}
			return text == "" ? "none" : text
		}
		# Least, middle and most of the values counted in bins of 25 us
		# under the prefix.
		function spread(table, prefix,    key, own, list, n, i, total, seen, middle) {
			total = 0
			for (key in table) {
				if (index(key, prefix) != 1) continue
				own[substr(key, length(prefix) + 1)] = table[key]
				total += table[key]
			}
			if (total == 0) return "none"
			n = in_order(own, list)
			seen = 0
			middle = list[1]
			for (i = 1; i <= n; i++) {
				seen += own[list[i]]
				if (seen * 2 >= total) { middle = list[i]; break }
			}
			return sprintf("least %d, half %d, most %d", list[1] * 25, middle * 25, list[n] * 25)
		}
		($3 == "S" || $3 == "C") && $4 ~ /^Z[io]:/ && index($4, device) {
			lines++
			point = $4
			if (!(point in known)) { known[point] = 1; order[++points] = point }
			if (first_time == "") first_time = $2
			last_time = $2
			packets = $6 + 0
			shown = packets > 5 ? 5 : packets
			bytes = $(7 + shown) + 0
			# For an input that came back the kernel gives the size of
			# the whole buffer: what really came is in the packets, of
			# which at most five are shown.
			if ($3 == "C" && $4 ~ /^Zi:/) {
				bytes = 0
				for (packet = 7; packet < 7 + shown; packet++) {
					split($packet, part, ":")
					bytes += part[3]
				}
			}
			silent = 1
			for (word = 9 + shown; word <= NF; word++) if ($word !~ /^0+$/) silent = 0
			split($5, state, ":")
			if ($3 == "S") {
				handed[point]++
				sent_at[$1] = $2
				if (!(point in first_handed)) first_handed[point] = $2
				if ($4 ~ /^Zo:/) {
					if (first_output == "") { first_output = $2; inputs_before_output = inputs_back }
					if (!silent && $(8 + shown) == "=" && first_sound == "") {
						first_sound = $2
						outputs_before_sound = handed[point] - 1
					}
					waiting[point] = handed[point] - back[point]
					kept_waiting[NR] = waiting[point]
					kept_point[NR] = point
					kept_time[NR] = $2
					kept++
				}
			} else {
				back[point]++
				if (!(point in first_back)) first_back[point] = $2
				if ($4 ~ /^Zi:/) {
					inputs_back++
					if (bytes > 0 && first_input_with_sound == "") { first_input_with_sound = $2; empty_inputs = inputs_back - 1 }
				}
				came[NR] = point
				came_time[NR] = $2
				came_packets[NR] = packets
				came_bytes[NR] = bytes
				came_stay[NR] = ($1 in sent_at) ? $2 - sent_at[$1] : -1
				# The microframes from the transfer before, on the same
				# endpoint: the lines come in the order of time.
				came_step[NR] = -1
				if (point in start_before) {
					came_step[NR] = state[3] - start_before[point]
					if (came_step[NR] < 0) came_step[NR] += 16384
				}
				start_before[point] = state[3]
				came_wrong[NR] = (state[1] != 0 || state[4] + 0 != 0)
				delete sent_at[$1]
			}
		}
		END {
			if (lines == 0) { print "  nothing of the card was on the bus"; exit }
			printf "  read: %d lines of the card in %.2f s\n", lines, (last_time - first_time) / 1000000
			from = last_time - window
			# The steady second: the last one read.
			for (line in came) {
				if (came_time[line] < from) continue
				point = came[line]
				steady[point]++
				count["packets " point " " came_packets[line]]++
				count["bytes " point " " came_bytes[line]]++
				if (came_stay[line] >= 0) count["stay " point " " int(came_stay[line] / 25)]++
				if (came_step[line] >= 0) count["step " point " " came_step[line]]++
				if (came_wrong[line]) wrong[point]++
			}
			for (line in kept_waiting) {
				if (kept_time[line] < from) continue
				count["waiting " kept_point[line] " " kept_waiting[line]]++
			}
			for (i = 1; i <= points; i++) {
				point = order[i]
				printf "  %s (%s): %d transfers came back in the last second read; in all %d handed over, %d back\n", point, (point ~ /^Zi/ ? "input" : "output"), steady[point], handed[point], back[point]
				printf "    packets in a transfer (times): %s\n", most_seen(count, "packets " point " ")
				printf "    bytes in a transfer%s (times): %s\n", (point ~ /^Zi/ ? ", in its first five packets at the most" : ""), most_seen(count, "bytes " point " ")
				printf "    microseconds with the controller, from handed over to back: %s\n", spread(count, "stay " point " ")
				printf "    microframes from a transfer to the next (times): %s\n", most_seen(count, "step " point " ")
				if (point ~ /^Zo/) printf "    output transfers with the controller when one more is handed over (times): %s\n", most_seen(count, "waiting " point " ")
				printf "    transfers that came back with an error: %d\n", wrong[point]
			}
			print "  the start, in milliseconds from the first line of the card:"
			for (i = 1; i <= points; i++) {
				point = order[i]
				printf "    %s: first handed over at %.3f, first back at %.3f\n", point, (first_handed[point] - first_time) / 1000, (first_back[point] - first_time) / 1000
			}
			if (first_input_with_sound != "") printf "    first input transfer that carried something: at %.3f, after %d empty ones\n", (first_input_with_sound - first_time) / 1000, empty_inputs
			if (first_output != "") printf "    first output transfer handed over: at %.3f, after %d input transfers had come back\n", (first_output - first_time) / 1000, inputs_before_output
			if (first_sound != "") printf "    first output transfer that was not silence: at %.3f, after %d of silence\n", (first_sound - first_time) / 1000, outputs_before_sound
			else print "    no output transfer carried anything but silence in its first 32 bytes"
		}' "$1"
}

# ---- The cases.
current_rate=
for rate in ${RATES}; do
	case "${rate}" in
	44100 | 48000) factor=1 ;;
	88200 | 96000) factor=2 ;;
	176400 | 192000) factor=4 ;;
	*) die "rate ${rate}: not one of the six of the engine" ;;
	esac
	frames=$((96 * factor))
	for slices in ${SLICES}; do
		name="${rate}-s${slices}"
		say ""
		say "-- ${rate} Hz, a quantum of ${frames} frames, ${slices} periods of the driver in it --"
		if [ -z "${TRY_COMMAND}" ] && [ "${rate}" != "${current_rate}" ]; then
			# The interface needs some seconds after a change of rate.
			"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --threads 1 --allow-xruns \
				--rate "${rate}" --seconds 8 --count 5:5 > "${dir}/settle-${rate}.txt" 2>&1
			current_rate=${rate}
		fi
		cat "${monitor}" > "${dir}/bus.raw" 2> /dev/null &
		reader=$!
		# The reader is there before the first transfer.
		sleep 1
		if [ -n "${TRY_COMMAND}" ]; then
			${TRY_COMMAND} > "${dir}/engine-${name}.txt" 2>&1 &
		else
			"${ENGINE}" run --backend alsa --device "hw:${identifier},0" --threads 1 --require-rt --allow-xruns \
				--rate "${rate}" --quantum "${frames}" --periods 3 --slices "${slices}" \
				--seconds $((READ_SECONDS + 3)) --count 5:5 > "${dir}/engine-${name}.txt" 2>&1 &
		fi
		runner=$!
		sleep "${READ_SECONDS}"
		# The streams as the driver sees them, while they run.
		cat "/proc/asound/card${card}/stream0" > "${dir}/stream-${name}.txt" 2> /dev/null
		kill "${reader}" 2> /dev/null
		wait "${reader}" 2> /dev/null
		if [ -n "${TRY_COMMAND}" ]; then
			kill "${runner}" 2> /dev/null
		fi
		wait "${runner}" 2> /dev/null

		read_bus "${dir}/bus.raw" ":${device}:" | tee -a "${dir}/summary.txt"
		# The lines themselves, cut short: how the streams start, and a
		# stretch of the steady part.
		awk -v device=":${device}:" '$4 ~ /^Z[io]:/ && index($4, device) { print substr($0, 1, 200) }' \
			"${dir}/bus.raw" > "${dir}/bus.card"
		sed -n '1,150p' "${dir}/bus.card" > "${dir}/bus-start-${name}.txt"
		tail -n 120 "${dir}/bus.card" > "${dir}/bus-steady-${name}.txt"
		rm -f "${dir}/bus.raw" "${dir}/bus.card"

		if [ -z "${TRY_COMMAND}" ]; then
			report="${dir}/engine-${name}.txt"
			say "  the engine, in the same run: way round $(field "${report}" count_on_input_5 | sed -n 's/^way round \([0-9]* samples ([0-9.]* ms)\).*/\1/p' | grep . || echo "not found"); xruns $(field "${report}" card_xruns | sed 's/ .*//'); $(sed -n 's/^  card_frames_at_the_wake_up: /frames at a wake-up: /p' "${report}" | head -n 1)"
			say "  $(sed -n 's/^  period_us: /length of a cycle, us: /p' "${report}" | head -n 1 | grep . || echo "length of a cycle: as asked")"
			grep -E '^(card_short_writes|card_driver_period_frames): ' "${report}" | sed 's/^/  /' | tee -a "${dir}/summary.txt"
		fi
	done
done

if [ "${routed}" = yes ]; then
	"${SETUP}" apply --card "${card}" > "${dir}/routing-after.txt" 2>&1
fi

# ---- What the kernel said meanwhile, about USB and sound.
dmesg | sed -n "$((kernel_lines_before + 1)),\$p" | grep -i -E 'xhci|usb|snd|alsa' > "${dir}/kernel-log.txt"
say ""
say "kernel log, lines about USB and sound written meanwhile: $(wc -l < "${dir}/kernel-log.txt")"
sed -n '1,12p' "${dir}/kernel-log.txt" | sed 's/^/  /' | tee -a "${dir}/summary.txt"

say ""
say "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
