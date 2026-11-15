#!/usr/bin/env bash
# dawOS post-image hook: runs after the filesystem images are created.
#
# Called by Buildroot as:  post-image.sh <BINARIES_DIR> <variant>
# with the Buildroot top directory as the working directory.
#
# It assembles two GPT disk images that boot through systemd-boot:
#
#   disk.img       the system as installed, for QEMU;
#   installer.img  the installation medium for the real machine (USB stick).
#
# Both carry the same root filesystem and kernel. They differ in the labels
# of the root partition and of its hash tree, so that a machine with the
# stick plugged in and dawOS already on its disk can never mount the wrong
# one, and in the "dawos.installer" marker on the kernel command line.
#
# The chain of trust built here, link by link:
#   firmware (Secure Boot)  checks the signature of the boot loader and of
#                           the unified kernel image;
#   unified kernel image    holds the kernel and its command line, hence
#                           the dm-verity root hash;
#   kernel (dm-verity)      checks every block read from the root.

set -euo pipefail

images_dir=${1:?usage: post-image.sh <BINARIES_DIR> <variant>}
variant=${2:-dev}

ext_dir=${BR2_EXTERNAL_DAWOS_PATH:?BR2_EXTERNAL_DAWOS_PATH is not set}
repo_dir=$(cd "${ext_dir}/.." && pwd)
board_dir=${ext_dir}/board/dawos/x86_64

# Size of one root slot, as on the real disk (document 0, section 3.3).
# The image file is sparse, so the unused part costs no space.
root_slot_size=4G
root_slot_bytes=$((4 * 1024 * 1024 * 1024))

version=$(tr -d '[:space:]' < "${repo_dir}/VERSION")
root_label="dawos-root-${version}"
verity_label="dawos-verity-${version}"
installer_label="dawos-inst-${version}"
installer_verity_label="dawos-instv-${version}"
# Size of the partition that holds the hash tree of one root slot. The
# tree takes a little less than 1/127 of the data: 128 MiB cover the
# whole 4 GiB slot with room to spare.
verity_slot_bytes=$((128 * 1024 * 1024))

die() {
	echo "post-image: $*" >&2
	exit 1
}

# Print the parameters of one or more command-line files on a single
# line: comments and blank lines dropped, whitespace collapsed.
read_cmdline() {
	grep -hEv '^[[:space:]]*(#|$)' "$@" | tr '\n' ' ' | sed -E 's/[[:space:]]+/ /g; s/ $//' || true
}

# --- Sanity checks ----------------------------------------------------------
[[ -f ${images_dir}/bzImage ]] || die "bzImage not found in ${images_dir}"
[[ -f ${images_dir}/rootfs.erofs ]] || die "rootfs.erofs not found in ${images_dir}"

# GPT partition names hold at most 36 characters.
for label in "${root_label}" "${verity_label}" "${installer_label}" "${installer_verity_label}"; do
	((${#label} <= 36)) || die "partition label '${label}' is longer than 36 characters"
done

root_bytes=$(stat -c %s "${images_dir}/rootfs.erofs")
((root_bytes <= root_slot_bytes)) || die "rootfs.erofs (${root_bytes} bytes) does not fit the ${root_slot_size} slot"

# --- Verified root (dm-verity) ------------------------------------------------
# veritysetup builds a tree of SHA-256 hashes over the root image: one
# hash per 4096-byte block, then hashes of those hashes, up to a single
# "root hash". The tree goes to its own partition; the root hash goes on
# the kernel command line. At run time the kernel checks every block it
# reads against the tree: a changed block is detected, and whoever can
# change the tree as well still cannot make it match the root hash.
veritysetup=${HOST_DIR:?HOST_DIR is not set}/sbin/veritysetup
[[ -x ${veritysetup} ]] || die "veritysetup not found (BR2_PACKAGE_HOST_CRYPTSETUP)"

verity_block=4096
((root_bytes % verity_block == 0)) || die "rootfs.erofs is not a whole number of ${verity_block}-byte blocks"
root_blocks=$((root_bytes / verity_block))
# Device-mapper counts in 512-byte sectors.
root_sectors=$((root_bytes / 512))

# Salt and UUID are derived from the image itself instead of being drawn
# at random: the same root image always gives the same tree and the same
# root hash, so a build can be reproduced and compared.
root_sha256=$(sha256sum "${images_dir}/rootfs.erofs" | cut -d ' ' -f 1)
verity_salt=${root_sha256}
verity_uuid="${root_sha256:0:8}-${root_sha256:8:4}-${root_sha256:12:4}-${root_sha256:16:4}-${root_sha256:20:12}"

rm -f "${images_dir}/rootfs.verity" "${images_dir}/rootfs.roothash"
"${veritysetup}" format \
	--hash=sha256 \
	--data-block-size="${verity_block}" --hash-block-size="${verity_block}" \
	--salt="${verity_salt}" --uuid="${verity_uuid}" \
	--root-hash-file="${images_dir}/rootfs.roothash" \
	"${images_dir}/rootfs.erofs" "${images_dir}/rootfs.verity" > "${images_dir}/rootfs.verity.txt"
root_hash=$(tr -d '[:space:]' < "${images_dir}/rootfs.roothash")
[[ ${root_hash} =~ ^[0-9a-f]{64}$ ]] || die "unexpected root hash '${root_hash}'"

verity_bytes=$(stat -c %s "${images_dir}/rootfs.verity")
((verity_bytes <= verity_slot_bytes)) || die "rootfs.verity (${verity_bytes} bytes) does not fit its slot"

# Print the kernel parameters that make the kernel build the verified
# root device by itself, without an initrd.
#   $1 = label of the data partition, $2 = label of the hash partition
#
# dm-mod.create="<name>,<uuid>,<minor>,<flags>,<table>" with the table
#   0 <sectors> verity <version> <data dev> <hash dev> <data block size>
#   <hash block size> <data blocks> <hash start block> <algorithm>
#   <root hash> <salt> <number of options> <options>
# - the devices are named by GPT label, which carries the version: the
#   same line works whichever slot the image is written to (PARTLABEL= in
#   this table is resolved by dm-init at boot: drivers/md/dm-table.c,
#   dm_devt_from_path, kernel 6.18);
# - hash start block 1: block 0 of the hash partition is the superblock
#   written by veritysetup, which the kernel does not use;
# - panic_on_corruption: a block that fails the check stops the machine
#   with a message instead of returning an I/O error to whoever read it;
# - dm-mod.waitfor: do not build the device before both partitions exist.
verity_params() {
	printf 'dm-mod.create="dawos-root,,,ro,0 %s verity 1 PARTLABEL=%s PARTLABEL=%s %s %s %s 1 sha256 %s %s 1 panic_on_corruption" dm-mod.waitfor=PARTLABEL=%s,PARTLABEL=%s root=/dev/dm-0' \
		"${root_sectors}" "$1" "$2" "${verity_block}" "${verity_block}" "${root_blocks}" \
		"${root_hash}" "${verity_salt}" "$1" "$2"
}

# --- Kernel command lines ---------------------------------------------------
cmdline_common=$(read_cmdline \
	"${board_dir}/cmdline/common.txt" \
	"${board_dir}/cmdline/profile-generic.txt")
cmdline="$(verity_params "${root_label}" "${verity_label}") ${cmdline_common}"
# The installation medium is verified too, under its own labels, so that a
# failing stick is noticed while it boots and not after it has been
# copied. "dawos.installer" marks the medium for dawos-install and for
# dawos-var-generator.
cmdline_installer="$(verity_params "${installer_label}" "${installer_verity_label}") ${cmdline_common} dawos.installer"
# Second menu entry of the installation medium, for bringing up a new
# machine: the whole boot on the screen.
#   earlycon=efifb  draws kernel messages straight on the firmware
#                   framebuffer, before any driver is up;
#   keep_bootcon    keeps that early console for the whole boot. Without
#                   it the kernel switches it off as soon as the virtual
#                   terminal exists, long before a graphics driver can
#                   show anything: the screen then looks frozen;
#   systemd.log_*   sends systemd's own messages through the kernel log,
#                   so they are drawn by the same early console.
# The framebuffer console still comes up when it can; the two then write
# on the same screen. Ugly, but nothing is hidden.
cmdline_installer_verbose="${cmdline_installer} ignore_loglevel earlycon=efifb keep_bootcon systemd.log_target=kmsg systemd.log_level=info"

# --- Secure Boot signing key (dev variant) --------------------------------------
# One self-signed certificate signs everything the firmware loads: the
# boot loader and the unified kernel images. It lives in keys/, which is
# never committed; it is created on the first build and then reused, so
# that a machine whose firmware trusts it keeps trusting later builds.
# This is a DEVELOPMENT key: anyone who holds keys/ can sign an image the
# machine will boot. The key of released images is a later decision.
openssl=${HOST_DIR}/bin/openssl
[[ -x ${openssl} ]] || die "host openssl not found (BR2_LINUX_KERNEL_NEEDS_HOST_OPENSSL)"
command -v sbsign > /dev/null || die "sbsign not found: sudo apt install sbsigntool"
command -v sbverify > /dev/null || die "sbverify not found: sudo apt install sbsigntool"

sb_dir=${repo_dir}/keys/secure-boot
sb_key=${sb_dir}/db.key
sb_cert=${sb_dir}/db.crt
if [[ ! -f ${sb_key} || ! -f ${sb_cert} ]]; then
	[[ ${variant} == dev ]] || die "no Secure Boot key in ${sb_dir}, and only the dev variant creates one"
	echo "post-image: creating the development Secure Boot key in ${sb_dir}"
	install -d -m 0700 "${sb_dir}"
	# RSA 2048 with SHA-256 is what every UEFI firmware accepts.
	(umask 077 && "${openssl}" req -new -x509 -newkey rsa:2048 -sha256 -nodes \
		-days 3650 -subj "/CN=dawOS development Secure Boot key/" \
		-keyout "${sb_key}" -out "${sb_cert}" 2> /dev/null)
fi
# The same certificate in binary form (DER): the format firmware setup
# menus read when a key is enrolled from a file.
"${openssl}" x509 -in "${sb_cert}" -outform DER -out "${sb_dir}/db.cer"
sb_fingerprint=$("${openssl}" x509 -in "${sb_cert}" -noout -fingerprint -sha256 | cut -d = -f 2)

# --- Unified kernel images ------------------------------------------------------
# A unified kernel image (UKI) is one EFI program holding the kernel, its
# command line and the description of the system, glued to a small loader
# (systemd-stub). It is signed as a whole: with Secure Boot on, the
# firmware refuses it if a single byte changed, and the stub ignores any
# command line coming from outside. That is what protects the dm-verity
# root hash, and through it the whole root file system.
ukify=(env "PATH=${HOST_DIR}/bin:${PATH}" "${HOST_DIR}/bin/python3" "${HOST_DIR}/bin/ukify")
[[ -x ${HOST_DIR}/bin/ukify ]] || die "ukify not found in ${HOST_DIR}/bin (host-systemd)"
efi_dir=${TARGET_DIR:?TARGET_DIR is not set}/usr/lib/systemd/boot/efi
[[ -f ${efi_dir}/linuxx64.efi.stub ]] || die "systemd-stub not found (BR2_PACKAGE_SYSTEMD_BOOT)"
[[ -f ${efi_dir}/systemd-bootx64.efi ]] || die "systemd-boot not found (BR2_PACKAGE_SYSTEMD_BOOT)"

uki_dir=${images_dir}/uki
rm -rf "${uki_dir}"
mkdir -p "${uki_dir}"

# Build and sign one image.
#   $1 = output file, $2 = os-release file (name shown in the boot menu),
#   $3 = kernel command line, $4... = further ukify arguments
build_uki() {
	local output=$1 os_release=$2 options=$3
	shift 3
	"${ukify[@]}" build \
		--linux="${images_dir}/bzImage" \
		--stub="${efi_dir}/linuxx64.efi.stub" \
		--os-release="@${os_release}" \
		--cmdline="${options}" \
		--signtool=sbsign \
		--secureboot-private-key="${sb_key}" \
		--secureboot-certificate="${sb_cert}" \
		--output="${output}" "$@" >> "${uki_dir}/ukify.log" 2>&1 ||
		die "ukify failed, see ${uki_dir}/ukify.log"
	sbverify --cert "${sb_cert}" "${output}" >> "${uki_dir}/ukify.log" 2>&1 ||
		die "the signature of ${output} does not verify"
}

# The installed system. The file name follows the Boot Loader
# Specification for unified images: <id>_<version>.efi.
uki_name="dawos_${version}.efi"
build_uki "${uki_dir}/${uki_name}" "${TARGET_DIR}/usr/lib/os-release" "${cmdline}"

# The installation medium: same kernel, its own command line and name,
# plus a second "profile" for the verbose boot. A profile is a named
# variant of the command line inside the same signed image; the boot
# loader lists each profile as a menu entry. (The same mechanism will
# carry the per-machine tuning profiles of step P0.6.)
sed -e "s/^PRETTY_NAME=.*/PRETTY_NAME=\"dawOS ${version} (${variant}) - installation medium\"/" \
	"${TARGET_DIR}/usr/lib/os-release" > "${uki_dir}/os-release-installer"
"${ukify[@]}" build \
	--stub="${efi_dir}/addonx64.efi.stub" \
	--profile=$'ID=verbose\nTITLE=verbose (all kernel messages on screen)' \
	--cmdline="${cmdline_installer_verbose}" \
	--output="${uki_dir}/profile-verbose.efi" >> "${uki_dir}/ukify.log" 2>&1 ||
	die "ukify failed, see ${uki_dir}/ukify.log"
installer_uki_name="dawos-installer_${version}.efi"
build_uki "${uki_dir}/${installer_uki_name}" "${uki_dir}/os-release-installer" "${cmdline_installer}" \
	--join-profile="${uki_dir}/profile-verbose.efi"

# The boot loader is the first thing the firmware checks: signed too.
sbsign --key "${sb_key}" --cert "${sb_cert}" \
	--output "${uki_dir}/bootx64.efi" "${efi_dir}/systemd-bootx64.efi" >> "${uki_dir}/ukify.log" 2>&1 ||
	die "signing systemd-boot failed, see ${uki_dir}/ukify.log"
# --- EFI System Partitions -------------------------------------------------------
# Fill the tree of one EFI System Partition: the signed boot loader, its
# configuration, and one unified image in EFI/Linux, where systemd-boot
# finds it by itself (Boot Loader Specification, type 2: no entry file).
#   $1 = directory, $2 = unified image
populate_esp() {
	local dir=$1 uki=$2

	rm -rf "${dir}"
	install -D -m 0644 "${uki_dir}/bootx64.efi" "${dir}/EFI/BOOT/bootx64.efi"
	install -D -m 0644 "${uki}" "${dir}/EFI/Linux/${uki##*/}"
	install -d -m 0755 "${dir}/loader"
	cat > "${dir}/loader/loader.conf" <<LOADER
# dawOS (${variant}): show the menu briefly so an entry can be picked by hand.
timeout 5
console-mode keep
# Keys are enrolled by hand in the firmware, never by the boot loader.
secure-boot-enroll off
LOADER
}

esp_dir=${images_dir}/efi-part
populate_esp "${esp_dir}" "${uki_dir}/${uki_name}"

inst_dir=${images_dir}/efi-inst
populate_esp "${inst_dir}" "${uki_dir}/${installer_uki_name}"
# What dawos-install needs to set up the disk travels with the medium:
# - the unified image of the system to install;
# - its command line in clear text, for the installer to show and to
#   take the root hash from, to check what it has copied (the copy that
#   counts is the one inside the signed image);
# - the certificate to enrol in the firmware of the machine.
install -D -m 0644 "${uki_dir}/${uki_name}" "${inst_dir}/dawos/${version}/${uki_name}"
printf '%s\n' "${cmdline}" > "${inst_dir}/dawos/${version}/options-installed"
install -D -m 0644 "${sb_dir}/db.cer" "${inst_dir}/dawos/secure-boot/dawos-dev-secure-boot.cer"

# Keep a copy next to the images: it is what the measurement reports cite.
printf '%s\n' "${cmdline}" > "${images_dir}/cmdline.txt"

# --- Disk images ------------------------------------------------------------
sed -e "s/@VERSION@/${version}/g" \
	-e "s/@ROOT_SLOT_SIZE@/${root_slot_size}/g" \
	"${board_dir}/genimage.cfg.in" > "${images_dir}/genimage.cfg"

support/scripts/genimage.sh -c "${images_dir}/genimage.cfg"

echo "post-image: ${images_dir}/disk.img ready (dawOS ${version}, ${variant})"
echo "post-image: ${images_dir}/installer.img ready (installation medium)"
echo "post-image: root hash: ${root_hash}"
echo "post-image: unified kernel image: ${uki_dir}/${uki_name} (signed)"
echo "post-image: Secure Boot certificate: ${sb_dir}/db.crt, SHA-256 fingerprint ${sb_fingerprint}"
echo "post-image: kernel command line: ${cmdline}"
