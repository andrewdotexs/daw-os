#!/bin/sh
# boot-compare.sh — what one boot of the machine left in the journal, or
# a wide reading of the registers of the processor: material to tell two
# boots apart. Reading only.
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s -- boots         < tools/boot-compare.sh
#   ssh "root@${H}" sh -s -- log BOOT      < tools/boot-compare.sh > FILE
#   ssh "root@${H}" sh -s -- registers     < tools/boot-compare.sh > FILE
#
# Why: the whole campaign of step P0.7 ran on one boot, and on that boot
# the machine behaved differently from every boot measured since (open
# points A-40 and A-43): wake-ups up to 20 us instead of 6, efficiency
# cores a little below their frequency, a cooler package. The image, the
# firmware version and the settings dawos-tune checks were the same. The
# journal on the disk still holds the kernel log of that boot.
#
# BOOT is what "journalctl -b" takes: an offset (0 is the current boot,
# -1 the one before) or the identifier printed by "boots". Messages come
# without their time stamps, so that two boots can be compared line by
# line; addresses are masked.

set -u

PROG="boot-compare.sh"

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

mask() {
	# Four numbers with dots and nothing numeric around them: a version
	# such as 0.0.1.20261117231225 is not an address.
	sed -E 's/([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}/[address]/g; s/(^|[^0-9.])([0-9]{1,3}\.){3}[0-9]{1,3}([^0-9.]|$)/\1[address]\3/g; s/[fF][eE]80:[0-9a-fA-F:]+/[address]/g'
}

# The 64 bits of register $2 on CPU $1, in hexadecimal; "-" where the
# processor has none.
msr() {
	value=$(dd if="/dev/cpu/$1/msr" bs=8 skip=$(($2)) count=1 iflag=skip_bytes 2> /dev/null | od -An -tx8 | tr -d ' \n')
	echo "${value:--}"
}

case "${1:-}" in
boots)
	journalctl --list-boots --no-pager 2> /dev/null
	;;
log)
	boot=${2:-}
	[ -n "${boot}" ] || die "log needs a boot: an offset or an identifier (see 'boots')"
	journalctl -b "${boot}" -n 0 --no-pager > /dev/null 2>&1 || die "the journal has no boot '${boot}'"
	echo "== boot ${boot}: first and last entry =="
	journalctl -b "${boot}" --no-pager -o short-iso 2> /dev/null | sed -n '1p' | cut -c 1-25
	journalctl -b "${boot}" --no-pager -o short-iso -n 1 2> /dev/null | tail -n 1 | cut -c 1-25
	echo
	echo "== kernel log =="
	journalctl -k -b "${boot}" --no-pager -o cat 2> /dev/null | mask | head -n 4000
	echo
	echo "== dawos-tune =="
	journalctl -b "${boot}" -u dawos-tune --no-pager -o cat 2> /dev/null | mask | head -n 200
	;;
registers)
	# One performance core and one efficiency core, neither of them
	# live: reading a register on a CPU interrupts it.
	live=$(sed -n 's/^ *"live": "\([0-9,-]*\)".*/\1/p' /run/dawos/topology.json 2> /dev/null | head -n 1)
	system=$(sed -n 's/^ *"system": "\([0-9]*\).*/\1/p' /run/dawos/topology.json 2> /dev/null | head -n 1)
	echo "== when and what =="
	date -u '+read_utc: %Y-%m-%dT%H:%M:%SZ'
	echo "uptime_s: $(cut -d. -f1 /proc/uptime)"
	echo "boot_id: $(tr -d '-' < /proc/sys/kernel/random/boot_id)"
	echo "bios: $(cat /sys/class/dmi/id/bios_version 2> /dev/null) $(cat /sys/class/dmi/id/bios_date 2> /dev/null)"
	echo "live_cpus: ${live:-unknown}"
	echo
	echo "== registers: number, value on cpu 0, value on cpu ${system:-16} =="
	for register in 0x17 0x1b 0x3a 0x8b 0xce 0xe2 0xe4 0x10a 0x13c 0x194 0x198 0x199 0x19a 0x19b 0x19c \
		0x19d 0x1a0 0x1a2 0x1a4 0x1a6 0x1aa 0x1ad 0x1ae 0x1af 0x1b0 0x1b1 0x1b2 0x1fc 0x34 \
		0x601 0x603 0x606 0x60a 0x60b 0x60c 0x60d 0x610 0x614 0x618 0x61c 0x620 0x621 0x638 0x63a \
		0x640 0x642 0x648 0x649 0x64a 0x64b 0x64c 0x64d 0x64e 0x64f 0x650 0x65c 0x690 0x6b0 0x6b1 \
		0x770 0x771 0x772 0x773 0x774 0x777; do
		echo "${register} $(msr 0 "${register}") $(msr "${system:-16}" "${register}")"
	done
	echo
	echo "== frequency settings the kernel shows, cpu by cpu =="
	for directory in /sys/devices/system/cpu/cpu[0-9]*/cpufreq; do
		[ -d "${directory}" ] || continue
		number=${directory%/cpufreq}
		number=${number##*/cpu}
		echo "cpu${number}: driver $(cat "${directory}/scaling_driver" 2> /dev/null), governor $(cat "${directory}/scaling_governor" 2> /dev/null), min $(cat "${directory}/scaling_min_freq" 2> /dev/null), max $(cat "${directory}/scaling_max_freq" 2> /dev/null), hardware max $(cat "${directory}/cpuinfo_max_freq" 2> /dev/null), preference $(cat "${directory}/energy_performance_preference" 2> /dev/null), base $(cat "${directory}/base_frequency" 2> /dev/null)"
	done | sort -t u -k 2 -n
	echo
	echo "== intel_pstate =="
	for file in /sys/devices/system/cpu/intel_pstate/*; do
		[ -f "${file}" ] && echo "${file##*/}: $(cat "${file}" 2> /dev/null)"
	done
	echo
	echo "== temperature sensors =="
	for hwmon in /sys/class/hwmon/hwmon*; do
		[ -r "${hwmon}/name" ] || continue
		echo "$(cat "${hwmon}/name"): $(cat "${hwmon}"/temp*_input 2> /dev/null | tr '\n' ' ')"
	done
	echo
	echo "== clock =="
	echo "clocksource: $(cat /sys/devices/system/clocksource/clocksource0/current_clocksource 2> /dev/null)"
	journalctl -k -b --no-pager -o cat 2> /dev/null | grep -i -E 'tsc:|clocksource:|microcode|intel_pstate|thermal|x86/cpu' | head -n 30
	;;
*)
	echo "usage: ${PROG} boots | log BOOT | registers" >&2
	exit 1
	;;
esac
