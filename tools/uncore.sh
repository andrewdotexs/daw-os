#!/bin/sh
# uncore.sh — read, fix or restore the frequency of the uncore (the ring
# and the L3 cache, shared by every core) of an Intel processor.
#
# It runs ON the machine, from the development PC:
#
#   ssh "root@${H}" sh -s -- --status  < tools/uncore.sh
#   ssh "root@${H}" sh -s -- --fix     < tools/uncore.sh
#   ssh "root@${H}" sh -s -- --restore < tools/uncore.sh
#
# Why: in the 24 hours of step P0.7 the live CPUs lost about 18 us, a
# handful of times, with nothing running on them (open point A-43). The
# firmware leaves the uncore free to change frequency between 0.8 and
# 3.8 GHz, and a change stops every core for a moment. Fixing the
# frequency, lowest ratio equal to highest, tells whether that is the
# cause.
#
# Nothing here survives a reboot: the limits are a register of the
# processor (MSR 0x620: highest ratio in bits 0-6, lowest in bits 8-14,
# in units of 100 MHz), written through /dev/cpu/N/msr. The kernel notes
# the write in its log and marks itself as tainted until the next boot.
# The register is read and written on one CPU of the system domain, so
# that no live CPU is interrupted.

set -u

PROG=uncore.sh
SAVED=/run/dawos-uncore-limits

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

cpu=$(sed -n 's/^ *"system": "\([0-9]*\).*/\1/p' /run/dawos/topology.json 2> /dev/null | head -n 1)
cpu=${cpu:-0}
msr=/dev/cpu/${cpu}/msr

# The value of register $1 as a decimal number; nothing if it cannot be
# read.
read_msr() {
	dd if="${msr}" bs=8 skip=$(($1)) count=1 iflag=skip_bytes 2> /dev/null | od -An -tu8 | tr -d ' \n'
}

# Write the number $2 to register $1: eight bytes, lowest first.
write_msr() {
	value=$2
	bytes=
	for _ in 1 2 3 4 5 6 7 8; do
		bytes="${bytes}\\$(printf '%03o' $((value & 255)))"
		value=$((value >> 8))
	done
	# The register number is a multiple of 8, so it is a whole number
	# of 8-byte blocks into the file.
	# shellcheck disable=SC2059 # the format is the bytes themselves
	printf "${bytes}" | dd of="${msr}" bs=8 seek=$(($1 / 8)) count=1 conv=notrunc 2> /dev/null
}

show() {
	limits=$(read_msr 0x620)
	[ -n "${limits}" ] || die "register 0x620 cannot be read on cpu ${cpu}: no uncore limits on this machine"
	[ "$((limits & 127))" -gt 0 ] || die "register 0x620 reads zero on cpu ${cpu}: this processor, or this virtual machine, has no uncore limits"
	echo "limits: lowest ratio $(((limits >> 8) & 127)), highest ratio $((limits & 127)) (register 0x620 = $(printf '0x%x' "${limits}"), read on cpu ${cpu})"
	printf 'current ratio, ten readings half a second apart:'
	for _ in 1 2 3 4 5 6 7 8 9 10; do
		status=$(read_msr 0x621)
		printf ' %s' "$((${status:-0} & 127))"
		sleep 0.5
	done
	echo
	if [ -r "${SAVED}" ]; then
		echo "limits saved before fixing: $(printf '0x%x' "$(cat "${SAVED}")")"
	fi
}

[ "$(id -u)" = 0 ] || die "must run as root"
[ -e "${msr}" ] || die "no ${msr}: the kernel does not expose the registers of the processor"

case "${1:-}" in
--status)
	show
	;;
--fix)
	limits=$(read_msr 0x620)
	[ -n "${limits}" ] || die "register 0x620 cannot be read on cpu ${cpu}"
	highest=$((limits & 127))
	[ "${highest}" -gt 0 ] || die "register 0x620 reads zero on cpu ${cpu}: nothing to fix here"
	ratio=${2:-${highest}}
	echo "${ratio}" | grep -Eq '^[0-9]{1,3}$' || die "bad ratio '${ratio}'"
	lowest_allowed=$(((limits >> 8) & 127))
	[ -r "${SAVED}" ] && lowest_allowed=$((($(cat "${SAVED}") >> 8) & 127))
	if [ "${ratio}" -lt "${lowest_allowed}" ] || [ "${ratio}" -gt "${highest}" ]; then
		die "ratio ${ratio} is outside the limits the firmware set (${lowest_allowed}-${highest})"
	fi
	# The limits of the firmware are kept once, to be put back.
	[ -r "${SAVED}" ] || echo "${limits}" > "${SAVED}"
	# Everything but the two ratios stays as it is.
	wanted=$(((limits & ~32639) | (ratio << 8) | ratio))
	write_msr 0x620 "${wanted}" || die "the processor refused the new limits"
	[ "$(read_msr 0x620)" = "${wanted}" ] || die "the limits did not change: this processor does not take them from here"
	echo "${PROG}: uncore fixed at ratio ${ratio} ($((ratio / 10)).$((ratio % 10)) GHz) until the next boot or --restore"
	show
	;;
--restore)
	[ -r "${SAVED}" ] || die "nothing to restore: the limits were not changed since boot"
	write_msr 0x620 "$(cat "${SAVED}")" || die "the processor refused the old limits"
	[ "$(read_msr 0x620)" = "$(cat "${SAVED}")" ] || die "the limits did not change back"
	rm -f "${SAVED}"
	echo "${PROG}: limits of the firmware restored"
	show
	;;
*)
	echo "usage: ${PROG} --status | --fix [RATIO] | --restore" >&2
	exit 1
	;;
esac
