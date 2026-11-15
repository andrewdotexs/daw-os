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
ab_test=
secure_boot=yes
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
  --tamper WHAT     with --test: change one byte in a copy of the image and
                    pass if the boot is stopped. WHAT is 'root' (the root
                    partition) or 'verity' (its hash tree), both caught by
                    the kernel; or 'uki' (the dm-verity root hash inside
                    the signed unified kernel image), caught by the
                    firmware
  --ab-test WHAT    with --test: boot the image that holds two system
                    images, the current one in slot A and a newer one in
                    slot B (output/images/ab-test.img). WHAT is 'good': the
                    newer image must be the one that boots; 'bad': slot B
                    is damaged first; 'unit': the newer image boots but a
                    unit fails in it. In the last two cases, after three
                    failed attempts the boot loader must fall back to
                    slot A
  --no-secure-boot  start the firmware with Secure Boot off
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
		tamper=${2:?--tamper needs 'root', 'verity' or 'uki'}
		shift
		;;
	--ab-test)
		ab_test=${2:?--ab-test needs 'good', 'bad' or 'unit'}
		shift
		;;
	--no-secure-boot) secure_boot=no ;;
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

# --- Changing one byte of an image --------------------------------------------
# tamper_image IMAGE WHAT PARTITION [ROOT_HASH]
#   PARTITION  GPT name of the partition to change; a trailing '*' takes
#              the first partition whose name starts with what precedes it
#   WHAT       root    one byte in the first block of a root file system
#                      (its superblock), certainly read while booting;
#              verity  one byte in the first block of a hash tree (its top
#                      level, on the path of every check). Block 0 of the
#                      hash partition is veritysetup's own superblock,
#                      unused by the kernel, hence the offset of one block;
#              uki     one digit of the dm-verity root hash ROOT_HASH
#                      inside the signed unified kernel image, on the EFI
#                      System Partition. The image is found by searching
#                      the partition for the hash.
tamper_image() {
	command -v python3 > /dev/null || die "python3 not found"
	python3 - "$@" <<'PYTHON' || die "could not tamper with $1"
import struct
import sys

path, what, selector = sys.argv[1:4]
root_hash = sys.argv[4] if len(sys.argv) > 4 else ""
with open(path, "r+b") as image:
    # GPT header in sector 1: signature, then at offset 72 the first
    # sector of the entry array, the number of entries and their size.
    image.seek(512)
    header = image.read(92)
    if header[:8] != b"EFI PART":
        sys.exit("no GPT in " + path)
    entries_lba, count, size = struct.unpack_from("<QII", header, 72)
    first = last = None
    for index in range(count):
        image.seek(entries_lba * 512 + index * size)
        entry = image.read(size)
        name = entry[56:128].decode("utf-16-le").rstrip("\x00")
        if name == selector or (selector.endswith("*") and name.startswith(selector[:-1])):
            first, last = struct.unpack_from("<QQ", entry, 32)
            break
    if first is None:
        sys.exit("no partition named " + selector + " in " + path)
    if what == "uki":
        # The command line is plain text inside the unified image.
        image.seek(first * 512)
        data = image.read((last - first + 1) * 512)
        needle = ("sha256 " + root_hash).encode()
        found = data.find(needle)
        if found < 0:
            sys.exit("root hash not found on the EFI System Partition")
        offset = first * 512 + found + len(needle) - 1
        image.seek(offset)
        old = image.read(1)[0]
        new = ord("0") if old != ord("0") else ord("1")
    else:
        offset = first * 512 + (1024 if what == "root" else 4096)
        image.seek(offset)
        old = image.read(1)[0]
        new = old ^ 0xFF
    image.seek(offset)
    image.write(bytes([new]))
print("run-qemu: tampered copy %s: byte %d of the image (partition %s) changed from %d to %d"
      % (path, offset, name, old, new))
PYTHON
}

# --- Tamper test: one changed byte in a copy of the image --------------------
if [[ -n ${tamper} ]]; then
	[[ ${mode} == test ]] || die "--tamper needs --test"
	[[ -z ${ab_test} ]] || die "--tamper and --ab-test cannot be combined"
	tampered=${state_dir}/tampered.img
	cp --sparse=always "${image}" "${tampered}"
	case ${tamper} in
	root) tamper_image "${tampered}" root 'dawos-root-*' ;;
	verity) tamper_image "${tampered}" verity 'dawos-verity-*' ;;
	uki)
		[[ ${secure_boot} == yes ]] || die "--tamper uki makes no sense with --no-secure-boot"
		[[ -f ${repo_dir}/output/images/rootfs.roothash ]] || die "root hash not found (run 'make image' first)"
		tamper_image "${tampered}" uki esp "$(tr -d '[:space:]' < "${repo_dir}/output/images/rootfs.roothash")"
		;;
	*) die "--tamper takes 'root', 'verity' or 'uki'" ;;
	esac
	image=${tampered}
fi

# --- A/B test: two system images, the newer one good, damaged or faulty --------
# Which boot report the test waits for. A machine that goes through
# several boots prints several: the one that counts names the expected
# root partition.
report_pattern='DAWOS-BOOT-REPORT .*'
if [[ -n ${ab_test} ]]; then
	[[ ${mode} == test ]] || die "--ab-test needs --test"
	ab_source=${repo_dir}/output/images/ab-test.img
	[[ -f ${ab_source} ]] || die "image not found: ${ab_source} (run 'make image' first)"
	version=$(tr -d '[:space:]' < "${repo_dir}/VERSION")
	slot_a="dawos-root-${version}"
	slot_b="dawos-root-${version}.abtest"
	image=${state_dir}/ab.img
	cp --sparse=always "${ab_source}" "${image}"
	case ${ab_test} in
	good)
		# The newer image, in slot B, is the one that must boot.
		expected_slot=${slot_b}
		;;
	bad)
		# Slot B is damaged: every attempt to boot it ends in a dm-verity
		# panic and a reboot. The one that completes must be slot A.
		tamper_image "${image}" root "${slot_b}"
		expected_slot=${slot_a}
		;;
	unit)
		# The image in slot B boots, but one unit fails in it: the marker
		# file of dawos-fail-boot-test.service is put on the "state"
		# partition. The partition is copied out, changed with debugfs
		# and copied back.
		debugfs=${repo_dir}/output/host/sbin/debugfs
		[[ -x ${debugfs} ]] || debugfs=$(command -v debugfs) || die "debugfs not found (run 'make image' first)"
		read -r state_start state_sectors < <(python3 - "${image}" <<'PYTHON'
import struct
import sys

with open(sys.argv[1], "rb") as image:
    image.seek(512)
    header = image.read(92)
    entries_lba, count, size = struct.unpack_from("<QII", header, 72)
    for index in range(count):
        image.seek(entries_lba * 512 + index * size)
        entry = image.read(size)
        if entry[56:128].decode("utf-16-le").rstrip("\x00") == "state":
            first, last = struct.unpack_from("<QQ", entry, 32)
            print(first, last - first + 1)
PYTHON
		)
		[[ ${state_start:-} =~ ^[0-9]+$ ]] || die "no state partition in ${image}"
		state_copy=${state_dir}/ab-state.ext4
		dd if="${image}" of="${state_copy}" bs=512 skip="${state_start}" count="${state_sectors}" status=none
		printf '%s\n' "${slot_b}" > "${state_dir}/fail-boot-on"
		"${debugfs}" -w "${state_copy}" > "${state_dir}/debugfs.log" 2>&1 <<DEBUGFS || die "debugfs failed, see ${state_dir}/debugfs.log"
mkdir lib
mkdir lib/dawos
mkdir lib/dawos/test
write ${state_dir}/fail-boot-on lib/dawos/test/fail-boot-on
DEBUGFS
		dd if="${state_copy}" of="${image}" bs=512 seek="${state_start}" conv=notrunc status=none
		rm -f "${state_copy}" "${state_dir}/fail-boot-on"
		echo "run-qemu: marker written: the image on ${slot_b} will fail a unit at every boot"
		expected_slot=${slot_a}
		;;
	*) die "--ab-test takes 'good', 'bad' or 'unit'" ;;
	esac
	expectations+=("rootpart=${expected_slot}")
	report_pattern="DAWOS-BOOT-REPORT .*rootpart=${expected_slot} .*"
fi

# --- UEFI firmware ----------------------------------------------------------
# OVMF comes as two files: read-only code and a variable store. The names
# differ between distributions; each line is "<code> <vars template>".
# With Secure Boot the build of the code that enforces it is needed (it
# also requires the emulated System Management Mode, see the machine
# options below); without it, the plain build.
ovmf_code=
ovmf_vars_template=
if [[ ${secure_boot} == yes ]]; then
	candidates='/usr/share/OVMF/OVMF_CODE_4M.secboot.fd /usr/share/OVMF/OVMF_VARS_4M.fd
/usr/share/edk2/x64/OVMF_CODE.secboot.4m.fd /usr/share/edk2/x64/OVMF_VARS.4m.fd
/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd /usr/share/edk2/ovmf/OVMF_VARS.fd'
else
	candidates='/usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_VARS_4M.fd
/usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF_VARS.fd
/usr/share/edk2/x64/OVMF_CODE.4m.fd /usr/share/edk2/x64/OVMF_VARS.4m.fd
/usr/share/edk2/ovmf/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_VARS.fd'
fi
while read -r code vars; do
	if [[ -f ${code} && -f ${vars} ]]; then
		ovmf_code=${code}
		ovmf_vars_template=${vars}
		break
	fi
done <<< "${candidates}"
[[ -n ${ovmf_code} ]] || die "OVMF firmware not found (package 'ovmf')"

# The variable store is written by the firmware (boot entries, keys): each
# checkout keeps its own private copy.
machine_options=
if [[ ${secure_boot} == yes ]]; then
	# A store with our development certificate enrolled and Secure Boot
	# switched on: the virtual counterpart of enrolling the certificate
	# in the setup menu of a real machine. One certificate plays all
	# three roles here (platform key, key exchange key, signature
	# database); only the last one matters for what may boot.
	sb_cert=${repo_dir}/keys/secure-boot/db.crt
	[[ -f ${sb_cert} ]] || die "Secure Boot certificate not found: ${sb_cert} (run 'make image' first)"
	command -v virt-fw-vars > /dev/null || die "virt-fw-vars not found: sudo apt install python3-virt-firmware"
	ovmf_vars=${state_dir}/OVMF_VARS.secboot.fd
	if [[ ! -f ${ovmf_vars} || ${sb_cert} -nt ${ovmf_vars} ]]; then
		# Owner GUID of the enrolled entries: any fixed value will do.
		owner=5a3c1e0e-9b6d-4c57-8f15-da0500000001
		virt-fw-vars --input "${ovmf_vars_template}" --output "${ovmf_vars}" \
			--set-pk "${owner}" "${sb_cert}" \
			--add-kek "${owner}" "${sb_cert}" \
			--add-db "${owner}" "${sb_cert}" \
			--secure-boot > "${state_dir}/virt-fw-vars.log" 2>&1 ||
			die "virt-fw-vars failed, see ${state_dir}/virt-fw-vars.log"
	fi
	# The variable store must be writable only from System Management
	# Mode, or the guest could rewrite the keys.
	machine_options=',smm=on'
else
	ovmf_vars=${state_dir}/OVMF_VARS.fd
	[[ -f ${ovmf_vars} ]] || cp "${ovmf_vars_template}" "${ovmf_vars}"
fi

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
	-machine "q35,accel=${accel}${machine_options}"
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

if [[ ${secure_boot} == yes ]]; then
	# Flash writable from System Management Mode only (see above).
	qemu_args+=(-global "driver=cfi.pflash01,property=secure,value=on")
fi

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

# A guest that reboots normally ends the test: nothing in a plain boot
# test should reboot. The A/B test is the exception, its whole point being
# a machine that restarts by itself until it finds an image that works.
reboot_option=(-no-reboot)
[[ -z ${ab_test} ]] || reboot_option=()

# QEMU's own messages go to a side file: on success they are just the
# notice that we stopped it; on failure they are printed below.
qemu_err=${state_dir}/qemu.err
# -snapshot: the guest is stopped abruptly once the report is seen, so
# its writes (boot loader random seed on the ESP) must not reach the image.
qemu-system-x86_64 "${qemu_args[@]}" \
	-display none -monitor none "${reboot_option[@]}" -snapshot \
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
	report=$(grep -a -m 1 -o "${report_pattern}" "${log}" | tr -d '\r' || true)
	[[ -n ${report} ]] && break
	if [[ ${tamper} == uki ]]; then
		# The firmware refuses to load the image; systemd-boot reports it.
		corruption=$(grep -a -m 1 -o -i -E '(access denied|security violation)' "${log}" || true)
		[[ -n ${corruption} ]] && break
	elif [[ -n ${tamper} ]]; then
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
		echo "FAIL: no boot report, but no message of refusal either; last lines of the console:" >&2
		tail -n 25 "${log}" | tr -d '\r' >&2
		exit 1
	fi
	# Give the kernel a moment to print what it does about it.
	sleep 2
	if [[ ${tamper} == uki ]]; then
		grep -a -m 1 -o -i -E 'Error loading EFI binary [^:]*: (access denied|security violation)' "${log}" ||
			echo "${corruption}"
		echo "PASS: the firmware refused the unified kernel image whose root hash was changed"
	else
		echo "${corruption}"
		grep -a -m 1 -o 'Kernel panic - not syncing: .*' "${log}" | tr -d '\r' || true
		echo "PASS: tampering with the ${tamper} partition was detected and the boot stopped"
	fi
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

if [[ ${ab_test} == bad || ${ab_test} == unit ]]; then
	# Every attempt on slot B must have ended badly, in one of two ways:
	# a kernel panic followed by a reboot (dm-verity for the damaged
	# image; any other panic counts as well, an attempt lost is an
	# attempt lost), or a reboot ordered by dawos-boot-failed after a
	# failed check. Three attempts in all, then slot A.
	panics=$(grep -a -c 'Kernel panic - not syncing' "${log}" || true)
	verity_panics=$(grep -a -c 'Kernel panic - not syncing: dm-verity device corrupted' "${log}" || true)
	ordered=$(grep -a -c -E 'dawOS: attempt [0-9]+ of [0-9]+ .* failed: rebooting' "${log}" || true)
	echo "failed attempts on slot B: $((panics + ordered)) (kernel panics: ${panics}, of which dm-verity: ${verity_panics}; reboots by dawos-boot-failed: ${ordered})"
	if ((panics + ordered != 3)); then
		echo "FAIL: expected exactly 3 failed attempts on slot B before the fallback" >&2
		echo "(console log: ${log})" >&2
		exit 1
	fi
	if [[ ${ab_test} == bad ]] && ((verity_panics == 0)); then
		echo "FAIL: the damaged image was never caught by dm-verity" >&2
		exit 1
	fi
	if [[ ${ab_test} == unit ]] && ((ordered == 0)); then
		echo "FAIL: dawos-boot-failed never ordered a reboot" >&2
		exit 1
	fi
	echo "PASS: after 3 failed attempts on slot B the boot loader fell back to slot A"
	exit 0
fi

echo "PASS: boot report matches: ${expectations[*]}"
