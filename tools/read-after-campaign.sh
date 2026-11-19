#!/bin/sh
# read-after-campaign.sh — what the machine says about itself after the
# campaign of step P0.7: reading only, nothing is changed.
#
# It runs ON the machine, from the development PC, and its output is the
# file kept in the repository:
#
#   ssh "root@${H}" sh -s < tools/read-after-campaign.sh \
#       > docs/misure/fase-0/letture-dopo-campagna.txt
#
# It collects what the notes of 19 November 2026 ask for: the state of
# the firmware as the processor reports it (the reference for the check
# of open point A-48), what the kernel needs to defer interrupts between
# CPUs (A-49), where the USB controllers and the sound card are (A-50),
# and two readings for A-43.
#
# The registers of the processor are read on one CPU of the system
# domain: reading them on a live CPU would interrupt it. Serial numbers
# and addresses are never printed.

set -u

section() {
	echo
	echo "== $1 =="
}

# The CPU the registers are read on: the first of the system domain.
cpu=$(sed -n 's/^ *"system": "\([0-9]*\).*/\1/p' /run/dawos/topology.json 2> /dev/null | head -n 1)
cpu=${cpu:-0}

# The 64 bits of register $1 (a number, hexadecimal allowed) on that
# CPU, in hexadecimal; "not readable" where the processor has none.
msr() {
	value=$(dd if="/dev/cpu/${cpu}/msr" bs=8 skip=$(($1)) count=1 iflag=skip_bytes 2> /dev/null | od -An -tx8 | tr -d ' \n')
	echo "${value:-not readable}"
}

section "when and what"
date -u '+read_utc: %Y-%m-%dT%H:%M:%SZ'
echo "uptime_s: $(cut -d. -f1 /proc/uptime)"
echo "image_version: $(sed -n 's/^IMAGE_VERSION=//p' /etc/os-release)"
echo "kernel: $(uname -srv)"
echo "bios: $(cat /sys/class/dmi/id/bios_version 2> /dev/null) $(cat /sys/class/dmi/id/bios_date 2> /dev/null)"
echo "microcode: $(cat /sys/devices/system/cpu/cpu*/microcode/version 2> /dev/null | sort -u | tr '\n' ' ')"
echo "profile: $(sed -n 's/.*dawos\.profile=\([a-z0-9-]*\).*/\1/p' /proc/cmdline)"
echo "modules_loaded: $(($(wc -l < /proc/modules)))"

section "registers of the processor, read on cpu ${cpu}"
echo "0xE2  package C-state limit and lock (MSR_PKG_CST_CONFIG_CONTROL): $(msr 0xE2)"
echo "0x1FC power control, bit 1 is C1E (MSR_POWER_CTL): $(msr 0x1FC)"
echo "0x610 package power limits and lock (MSR_PKG_POWER_LIMIT): $(msr 0x610)"
echo "0x34  SMI count since power-on (MSR_SMI_COUNT): $(msr 0x34)"
echo "0x620 limits of the uncore frequency ratio (MSR_UNCORE_RATIO_LIMIT): $(msr 0x620)"
for reading in 1 2 3 4 5; do
	echo "0x621 current uncore frequency ratio, reading ${reading}: $(msr 0x621)"
	sleep 1
done

section "idle states the kernel offers"
echo "driver: $(cat /sys/devices/system/cpu/cpuidle/current_driver 2> /dev/null)"
for which in 0 4 8 16; do
	[ -d "/sys/devices/system/cpu/cpu${which}/cpuidle" ] || continue
	printf 'cpu %s:' "${which}"
	for state in "/sys/devices/system/cpu/cpu${which}/cpuidle"/state*; do
		[ -r "${state}/name" ] || continue
		printf ' %s(disabled=%s)' "$(cat "${state}/name")" "$(cat "${state}/disable" 2> /dev/null)"
	done
	echo
done

section "what the firmware leaves to the kernel (kernel log)"
journalctl -k -b --no-pager -o cat 2> /dev/null | grep -i -E '_OSC|ASPM|AER|CMCI|mce:|machine check' | head -n 30

section "vulnerabilities and page-table isolation"
for file in /sys/devices/system/cpu/vulnerabilities/*; do
	[ -r "${file}" ] && echo "${file##*/}: $(cat "${file}")"
done
echo "pti flag of the processor: $(grep -m 1 -o -w pti /proc/cpuinfo || echo absent)"
journalctl -k -b --no-pager -o cat 2> /dev/null | grep -i 'page tables isolation' | head -n 2

section "kernel configuration, the options that matter here"
zcat /proc/config.gz 2> /dev/null | grep -E '^(# )?CONFIG_(MITIGATION_PAGE_TABLE_ISOLATION|NO_HZ_FULL|CONTEXT_TRACKING_USER|GENERIC_SYSCALL|X86_MSR|X86_BUS_LOCK_DETECT|INTEL_UNCORE_FREQ_CONTROL|X86_CPU_RESCTRL|TRANSPARENT_HUGEPAGE|COMPACTION|SND_USB_AUDIO|HZ)[ =]'

section "memory settings"
echo "transparent_hugepage: $(cat /sys/kernel/mm/transparent_hugepage/enabled 2> /dev/null || echo 'not in this kernel')"
echo "compact_unevictable_allowed: $(cat /proc/sys/vm/compact_unevictable_allowed 2> /dev/null)"
echo "stat_interval: $(cat /proc/sys/vm/stat_interval 2> /dev/null)"

# The machine was restarted after the campaign: what the kernel said
# during it is in the log of an earlier boot. Every boot the journal
# still holds is searched; the line each boot prints about how split
# locks are treated is shown once and left out of the count.
section "bus locks seen by the kernel, in every boot the journal holds"
journalctl -k --no-pager -o cat 2> /dev/null | grep -i -m 1 'split lock detection: #AC: crashing'
journalctl -k --no-pager -o short-iso 2> /dev/null | grep -i -E 'split.?lock|bus.?lock' | grep -v -i 'crashing the kernel on kernel split_locks' | head -n 20
echo "lines: $(journalctl -k --no-pager -o cat 2> /dev/null | grep -i -E 'split.?lock|bus.?lock' | grep -v -i -c 'crashing the kernel on kernel split_locks')"

section "boots the journal holds"
journalctl --list-boots --no-pager 2> /dev/null | tail -n 6

# How the boot before this one ended: the user reports that "poweroff"
# leaves the machine on. Lines of the system manager and of the kernel
# only, with any address masked.
section "the last lines of the previous boot"
journalctl -b -1 --no-pager -o short-iso 2> /dev/null |
	grep -E ' (systemd|systemd-shutdown|systemd-logind|kernel)(\[[0-9]+\])?: ' |
	sed -E 's/([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}/[address]/g; s/[0-9]{1,3}(\.[0-9]{1,3}){3}/[address]/g' |
	tail -n 25

section "USB: which controller each bus belongs to"
for bus in /sys/bus/usb/devices/usb*; do
	[ -d "${bus}" ] || continue
	echo "${bus##*/}: $(readlink -f "${bus}" | sed -n 's|.*/\(0000:[0-9a-f:.]*\)/usb[0-9]*$|\1|p'), $(cat "${bus}/speed" 2> /dev/null) Mbit/s"
done

section "USB: the tree"
lsusb -t 2> /dev/null

section "USB: the devices (no serial numbers)"
for device in /sys/bus/usb/devices/[0-9]*-[0-9.]*; do
	case "${device}" in
	*:*) continue ;;
	esac
	[ -r "${device}/idVendor" ] || continue
	echo "${device##*/}: $(cat "${device}/idVendor"):$(cat "${device}/idProduct") $(cat "${device}/manufacturer" 2> /dev/null) / $(cat "${device}/product" 2> /dev/null), $(cat "${device}/speed" 2> /dev/null) Mbit/s"
done

section "USB controllers: interrupts and the CPUs they may reach"
grep -E 'xhci' /proc/interrupts | while read -r line; do
	irq=${line%%:*}
	irq=$(echo "${irq}" | tr -d ' ')
	total=$(echo "${line}" | awk '{ sum = 0; for (i = 2; i <= NF; i++) if ($i ~ /^[0-9]+$/) sum += $i; print sum }')
	where=$(echo "${line}" | awk '{ for (i = 2; i <= NF; i++) if ($i ~ /^[0-9]+$/ && $i > 0) printf "cpu%d=%s ", i - 2, $i }')
	echo "irq ${irq}: $(echo "${line}" | awk '{ print $NF }'), total ${total}, allowed on $(cat "/proc/irq/${irq}/smp_affinity_list" 2> /dev/null), effective $(cat "/proc/irq/${irq}/effective_affinity_list" 2> /dev/null); counted on: ${where:-none}"
done

section "sound cards"
cat /proc/asound/cards 2> /dev/null
for card in /proc/asound/card[0-9]*; do
	[ -r "${card}/usbid" ] || continue
	echo "${card##*/}: usb id $(cat "${card}/usbid")"
	[ -r "${card}/stream0" ] && sed 's/^/  /' "${card}/stream0" | head -n 60
done

section "graphics cards and their drivers"
lspci -nnk 2> /dev/null | grep -A 3 -E 'VGA compatible|3D controller|Audio device' | grep -v -i 'subsystem'
echo "modules named nvidia or nouveau: $(grep -c -E '^(nvidia|nouveau)' /proc/modules)"

section "interrupt counters of the live CPUs since boot"
live=$(sed -n 's/^ *"live": "\([0-9,-]*\)".*/\1/p' /run/dawos/topology.json 2> /dev/null | head -n 1)
echo "live_cpus: ${live:-unknown}"
awk -v live="${live}" '
	BEGIN {
		count = split(live, part, ",")
		for (i = 1; i <= count; i++) {
			n = split(part[i], edge, "-")
			for (c = edge[1]; c <= edge[n]; c++) wanted[c] = 1
		}
	}
	$1 == "CAL:" || $1 == "TLB:" || $1 == "RES:" || $1 == "IWI:" || $1 == "NMI:" || $1 == "MCE:" || $1 == "MCP:" || $1 == "THR:" || $1 == "TRM:" {
		printf "%s", $1
		for (c = 0; c < NF - 1; c++) if (c in wanted) printf " cpu%d=%s", c, $(c + 2)
		printf "\n"
	}' /proc/interrupts
