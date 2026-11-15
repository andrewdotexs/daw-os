#!/usr/bin/env bash
# Boot dawOS in QEMU with UEFI firmware (OVMF).
#
# Interactive mode (default): serial console on this terminal.
#   Leave with Ctrl-A then X.
# Installation rehearsal (--installer): boots the installation medium as a
#   USB stick next to a blank NVMe disk, to run dawos-install exactly as
#   on the real machine. --target then boots the disk it installed.
# Test mode (--test): headless boot, waits for the boot report printed by
#   dawos-boot-report.service and exits 0 (PASS) or 1 (FAIL). The report
#   must say failed_units=0 and realtime=1, plus any --expect KEY=VALUE.
#
# QEMU is for functional checks only (boot chain, services, A/B updates).
# Latency figures measured here are meaningless: see document 0, 3.6.

set -euo pipefail

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

image=${repo_dir}/output/images/disk.img
installer_image=${repo_dir}/output/images/installer.img
state_dir=${repo_dir}/output/qemu
# Blank disk used by the installation rehearsal. Sparse: it only takes
# the space actually written. Sized like the disk of the reference machine.
target_image=${state_dir}/target.img
target_size=932G
installer=no
new_target=no
tamper=
mode=interactive
memory=2G
cpus=4
ssh_port=2222
timeout_s=300
# Fields the boot report must contain for the test to pass.
expectations=(failed_units=0 realtime=1)

usage() {
	cat <<USAGE
Usage: ${0##*/} [options]

  --test            headless boot; check the boot report; exit 0/1
  --expect K=V      with --test: also require this field in the boot report
  --tamper WHAT     with --test: change one byte of the 'root' partition or
                    of its 'verity' hash tree in a copy of the image; the
                    test passes if the kernel notices and the boot stops
  --image PATH      disk image to boot (default: output/images/disk.img)
  --installer       boot the installation medium (USB) with a blank NVMe
                    target disk, to rehearse dawos-install
  --new-target      with --installer: start again from a blank target disk
  --target          boot the disk installed by the rehearsal
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
	--expect)
		expectations+=("${2:?--expect needs KEY=VALUE}")
		shift
		;;
	--tamper)
		tamper=${2:?--tamper needs 'root' or 'verity'}
		shift
		;;
	--image)
		image=${2:?--image needs a path}
		shift
		;;
	--installer) installer=yes ;;
	--new-target) new_target=yes ;;
	--target) image=${target_image} ;;
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

mkdir -p "${state_dir}"

if [[ ${installer} == yes ]]; then
	[[ ${mode} == interactive ]] || die "--installer is interactive: it cannot be combined with --test"
	[[ -f ${installer_image} ]] || die "image not found: ${installer_image} (run 'make image' first)"
	if [[ ${new_target} == yes ]]; then
		rm -f "${target_image}"
	fi
	if [[ ! -f ${target_image} ]]; then
		truncate -s "${target_size}" "${target_image}"
		echo "run-qemu: created a blank ${target_size} target disk (sparse) at ${target_image}" >&2
	fi
	image=${target_image}
fi

[[ -f ${image} ]] || die "image not found: ${image} (run 'make image' or, for --target, the rehearsal first)"

# --- Tamper test: one changed byte in a copy of the image --------------------
if [[ -n ${tamper} ]]; then
	[[ ${mode} == test ]] || die "--tamper needs --test"
	[[ ${tamper} == root || ${tamper} == verity ]] || die "--tamper takes 'root' or 'verity'"
	command -v python3 > /dev/null || die "python3 not found"

	tampered=${state_dir}/tampered.img
	cp --sparse=always "${image}" "${tampered}"

	# Find the partition whose name starts with dawos-root- or
	# dawos-verity- in the GPT of the image itself, and invert one byte
	# that is certainly read while booting: in the first block of the
	# root file system (its superblock), or in the first block of the
	# hash tree (its top level, on the path of every check). Block 0 of
	# the hash partition is veritysetup's own superblock, unused by the
	# kernel, hence the offset of one block.
	python3 - "${tampered}" "dawos-${tamper}-" "${tamper}" <<'PYTHON' || die "could not tamper with ${tampered}"
import struct
import sys

path, prefix, what = sys.argv[1:4]
with open(path, "r+b") as image:
    # GPT header in sector 1: signature, then at offset 72 the first
    # sector of the entry array, the number of entries and their size.
    image.seek(512)
    header = image.read(92)
    if header[:8] != b"EFI PART":
        sys.exit("no GPT in " + path)
    entries_lba, count, size = struct.unpack_from("<QII", header, 72)
    start = None
    for index in range(count):
        image.seek(entries_lba * 512 + index * size)
        entry = image.read(size)
        name = entry[56:128].decode("utf-16-le").rstrip("\x00")
        if name.startswith(prefix):
            start = struct.unpack_from("<Q", entry, 32)[0]
            break
    if start is None:
        sys.exit("no partition named " + prefix + "* in " + path)
    offset = start * 512 + (1024 if what == "root" else 4096)
    image.seek(offset)
    old = image.read(1)[0]
    image.seek(offset)
    image.write(bytes([old ^ 0xFF]))
print("run-qemu: tampered copy %s: byte %d of the image (partition %s) changed from %d to %d"
      % (path, offset, name, old, old ^ 0xFF))
PYTHON
	image=${tampered}
fi

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
	# A fixed firmware UUID: dawOS derives its machine identity from it
	# (systemd.machine_id=firmware). QEMU's default is all zeros, which
	# systemd rejects, falling back to a new random identity per boot.
	-uuid 0d6f1a52-9c3e-4b7d-8a41-5e2c7f903b16
	# UEFI firmware: read-only code, private variable store.
	-drive "if=pflash,format=raw,unit=0,readonly=on,file=${ovmf_code}"
	-drive "if=pflash,format=raw,unit=1,file=${ovmf_vars}"
	# The system disk is an emulated NVMe, as on the reference machine.
	# discard=unmap keeps a sparse image sparse when the guest trims it.
	-drive "if=none,id=nvme0,format=raw,file=${image},discard=unmap"
	-device "nvme,drive=nvme0,serial=DAWOSQEMU0001,bootindex=2"
	# User-mode network with guest port 22 reachable on the loopback only.
	-netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${ssh_port}-:22"
	-device "virtio-net-pci,netdev=net0"
	-device virtio-rng-pci
)

if [[ ${installer} == yes ]]; then
	# The installation medium is a USB stick and comes first in the boot
	# order. snapshot=on: the image file itself is never modified.
	qemu_args+=(
		-device "qemu-xhci,id=xhci"
		-drive "if=none,id=stick,format=raw,file=${installer_image},snapshot=on"
		-device "usb-storage,drive=stick,bus=xhci.0,removable=on,bootindex=1"
	)
fi

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
# Kernel message of dm-verity when a block does not match the hash tree.
corruption=
deadline=$((SECONDS + timeout_s))
while ((SECONDS < deadline)); do
	# -a: the log contains terminal escape sequences from the firmware.
	report=$(grep -a -m 1 -o 'DAWOS-BOOT-REPORT .*' "${log}" | tr -d '\r' || true)
	[[ -n ${report} ]] && break
	if [[ -n ${tamper} ]]; then
		corruption=$(grep -a -m 1 -o -E 'device-mapper: verity: .* block [0-9]+ is corrupted' "${log}" || true)
		[[ -n ${corruption} ]] && break
	fi
	kill -0 "${qemu_pid}" 2> /dev/null || break
	sleep 1
done

# A tamper test passes when the boot does NOT complete and the kernel has
# said why.
if [[ -n ${tamper} ]]; then
	if [[ -n ${report} ]]; then
		echo "${report}"
		echo "FAIL: the system booted although the ${tamper} partition was tampered with" >&2
		exit 1
	fi
	if [[ -z ${corruption} ]]; then
		echo "FAIL: no boot report, but no dm-verity message either; last lines of the console:" >&2
		tail -n 25 "${log}" | tr -d '\r' >&2
		exit 1
	fi
	# Give the kernel a moment to print what it does about it.
	sleep 2
	echo "${corruption}"
	grep -a -m 1 -o 'Kernel panic - not syncing: .*' "${log}" | tr -d '\r' || true
	echo "PASS: tampering with the ${tamper} partition was detected and the boot stopped"
	exit 0
fi

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

# Each expectation must appear as a whole space-separated field.
failed=0
for expected in "${expectations[@]}"; do
	if [[ " ${report} " != *" ${expected} "* ]]; then
		echo "FAIL: boot report does not contain '${expected}'" >&2
		failed=1
	fi
done
if ((failed)); then
	echo "(console log: ${log})" >&2
	exit 1
fi

echo "PASS: boot report matches: ${expectations[*]}"
