#!/usr/bin/env bash
# Boot the dawOS disk image in QEMU with UEFI firmware (OVMF).
#
# Interactive mode (default): serial console on this terminal.
#   Leave with Ctrl-A then X.
# Test mode (--test): headless boot, waits for the boot report printed by
#   dawos-boot-report.service and exits 0 (PASS) or 1 (FAIL).
#
# QEMU is for functional checks only (boot chain, services, A/B updates).
# Latency figures measured here are meaningless: see document 0, 3.6.

set -euo pipefail

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

image=${repo_dir}/output/images/disk.img
state_dir=${repo_dir}/output/qemu
mode=interactive
memory=2G
cpus=4
ssh_port=2222
timeout_s=300

usage() {
	cat <<USAGE
Usage: ${0##*/} [options]

  --test            headless boot; check the boot report; exit 0/1
  --image PATH      disk image to boot (default: output/images/disk.img)
  --memory SIZE     guest memory (default: ${memory})
  --cpus N          guest CPUs (default: ${cpus})
  --ssh-port PORT   host port forwarded to guest port 22 (default: ${ssh_port})
  --timeout SECONDS time limit for --test (default: ${timeout_s})
  -h, --help        show this help
USAGE
}

die() {
	echo "run-qemu: $*" >&2
	exit 1
}

while (($# > 0)); do
	case $1 in
	--test) mode='test' ;;
	--image)
		image=${2:?--image needs a path}
		shift
		;;
	--memory)
		memory=${2:?--memory needs a size}
		shift
		;;
	--cpus)
		cpus=${2:?--cpus needs a number}
		shift
		;;
	--ssh-port)
		ssh_port=${2:?--ssh-port needs a port}
		shift
		;;
	--timeout)
		timeout_s=${2:?--timeout needs a number of seconds}
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*) die "unknown option '$1' (try --help)" ;;
	esac
	shift
done

command -v qemu-system-x86_64 > /dev/null || die "qemu-system-x86_64 not found (see docs/fasi/fase-0/P0.1-ambiente.md)"
[[ -f ${image} ]] || die "image not found: ${image} (run 'make image' first)"

# --- UEFI firmware ----------------------------------------------------------
# OVMF comes as two files: read-only code and a variable store. The names
# differ between distributions; each line is "<code> <vars template>".
ovmf_code=
ovmf_vars_template=
while read -r code vars; do
	if [[ -f ${code} && -f ${vars} ]]; then
		ovmf_code=${code}
		ovmf_vars_template=${vars}
		break
	fi
done <<'CANDIDATES'
/usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_VARS_4M.fd
/usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF_VARS.fd
/usr/share/edk2/x64/OVMF_CODE.4m.fd /usr/share/edk2/x64/OVMF_VARS.4m.fd
/usr/share/edk2/ovmf/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_VARS.fd
CANDIDATES
[[ -n ${ovmf_code} ]] || die "OVMF firmware not found (package 'ovmf')"

# The variable store is written by the firmware (boot entries, later the
# Secure Boot keys): each checkout keeps its own private copy.
mkdir -p "${state_dir}"
ovmf_vars=${state_dir}/OVMF_VARS.fd
[[ -f ${ovmf_vars} ]] || cp "${ovmf_vars_template}" "${ovmf_vars}"

# --- Acceleration -----------------------------------------------------------
# With KVM the guest sees the host CPU. Without it QEMU emulates one:
# "max" is needed because userspace is built for x86-64-v3 (AVX2, FMA).
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
	accel=kvm
	cpu_model=host
else
	accel=tcg
	cpu_model=max
	echo "run-qemu: /dev/kvm not usable, falling back to slow emulation (TCG)" >&2
fi

qemu_args=(
	-machine "q35,accel=${accel}"
	-cpu "${cpu_model}"
	-smp "${cpus}"
	-m "${memory}"
	# UEFI firmware: read-only code, private variable store.
	-drive "if=pflash,format=raw,unit=0,readonly=on,file=${ovmf_code}"
	-drive "if=pflash,format=raw,unit=1,file=${ovmf_vars}"
	# The system disk is an emulated NVMe, as on the reference machine.
	-drive "if=none,id=nvme0,format=raw,file=${image}"
	-device "nvme,drive=nvme0,serial=DAWOSQEMU0001"
	# User-mode network with guest port 22 reachable on the loopback only.
	-netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${ssh_port}-:22"
	-device "virtio-net-pci,netdev=net0"
	-device virtio-rng-pci
)

if [[ ${mode} == interactive ]]; then
	echo "run-qemu: booting ${image} (accel=${accel}); leave with Ctrl-A then X" >&2
	exec qemu-system-x86_64 "${qemu_args[@]}" -nographic
fi

# --- Test mode --------------------------------------------------------------
log=${state_dir}/serial.log
: > "${log}"

# QEMU's own messages go to a side file: on success they are just the
# notice that we stopped it; on failure they are printed below.
qemu_err=${state_dir}/qemu.err
# -snapshot: the guest is stopped abruptly once the report is seen, so
# its writes (boot loader random seed on the ESP) must not reach the image.
qemu-system-x86_64 "${qemu_args[@]}" \
	-display none -monitor none -no-reboot -snapshot \
	-serial "file:${log}" 2> "${qemu_err}" &
qemu_pid=$!

cleanup() {
	kill "${qemu_pid}" 2> /dev/null || true
	wait "${qemu_pid}" 2> /dev/null || true
}
trap cleanup EXIT

echo "run-qemu: waiting up to ${timeout_s}s for the boot report (accel=${accel}, log: ${log})"

report=
deadline=$((SECONDS + timeout_s))
while ((SECONDS < deadline)); do
	# -a: the log contains terminal escape sequences from the firmware.
	report=$(grep -a -m 1 -o 'DAWOS-BOOT-REPORT .*' "${log}" | tr -d '\r' || true)
	[[ -n ${report} ]] && break
	kill -0 "${qemu_pid}" 2> /dev/null || break
	sleep 1
done

if [[ -z ${report} ]]; then
	echo "FAIL: no boot report within ${timeout_s}s; last lines of the console:" >&2
	tail -n 25 "${log}" | tr -d '\r' >&2
	if [[ -s ${qemu_err} ]]; then
		echo "QEMU messages:" >&2
		cat "${qemu_err}" >&2
	fi
	exit 1
fi

echo "${report}"

if [[ ${report} != *" failed_units=0 "* ]]; then
	echo "FAIL: some systemd units failed (see ${log})" >&2
	exit 1
fi

echo "PASS: dawOS reached multi-user.target with no failed units"
