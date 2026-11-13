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
# Both carry the same root filesystem and kernel. They differ in the label
# of the root partition, so that a machine with the stick plugged in and
# dawOS already on its disk can never mount the wrong one, and in the
# "dawos.installer" marker on the kernel command line.
#
# There is no verity, no UKI and no signature yet; those arrive in step
# P0.5 and replace the "boot entry" sections below.

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
installer_label="dawos-inst-${version}"

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
[[ -f ${images_dir}/efi-part/EFI/BOOT/bootx64.efi ]] || die "systemd-boot not found (BR2_PACKAGE_SYSTEMD_BOOT)"

# GPT partition names hold at most 36 characters.
((${#root_label} <= 36)) || die "partition label '${root_label}' is longer than 36 characters"

root_bytes=$(stat -c %s "${images_dir}/rootfs.erofs")
((root_bytes <= root_slot_bytes)) || die "rootfs.erofs (${root_bytes} bytes) does not fit the ${root_slot_size} slot"

# --- Kernel command lines ---------------------------------------------------
# The root partition is named by its GPT label, which carries the version:
# the same command line works whichever slot the image is written to.
cmdline_common=$(read_cmdline \
	"${board_dir}/cmdline/common.txt" \
	"${board_dir}/cmdline/profile-generic.txt")
cmdline="root=PARTLABEL=${root_label} ${cmdline_common}"
# dawos-install derives the first form from the second: it swaps the root
# label back and cuts the line at the "dawos.installer" marker. Anything
# meant for the installation medium only must therefore come after it.
cmdline_installer="root=PARTLABEL=${installer_label} ${cmdline_common} dawos.installer"
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

# --- Boot entries (Boot Loader Specification, type 1) --------------------------
# Fill the tree of one EFI System Partition.
#   $1 = directory, $2 = menu title, $3 = kernel command line,
#   $4 = command line of an extra "verbose" entry (optional)
populate_esp() {
	local dir=$1 title=$2 options=$3 verbose_options=${4:-}

	install -D -m 0644 "${images_dir}/bzImage" "${dir}/dawos/${version}/linux"

	install -d -m 0755 "${dir}/loader/entries"
	cat > "${dir}/loader/loader.conf" <<LOADER
# dawOS (${variant}): show the menu briefly so an entry can be picked by hand.
timeout 5
default dawos-${version}.conf
console-mode keep
LOADER

	cat > "${dir}/loader/entries/dawos-${version}.conf" <<ENTRY
title   ${title}
version ${version}
sort-key dawos
linux   /dawos/${version}/linux
options ${options}
ENTRY

	if [[ -n ${verbose_options} ]]; then
		cat > "${dir}/loader/entries/dawos-${version}-verbose.conf" <<ENTRY
title   ${title}, verbose (all kernel messages on screen)
version ${version}
sort-key dawos-verbose
linux   /dawos/${version}/linux
options ${verbose_options}
ENTRY
	fi
}

# systemd-boot itself is installed by Buildroot in efi-part/EFI.
esp_dir=${images_dir}/efi-part
# Drop Buildroot's sample configuration and anything left by an earlier run.
rm -rf "${esp_dir}/loader" "${esp_dir}/dawos"
populate_esp "${esp_dir}" "dawOS ${version} (${variant})" "${cmdline}"

# The installation medium gets its own tree, with the same boot loader.
inst_dir=${images_dir}/efi-inst
rm -rf "${inst_dir}"
install -D -m 0644 "${esp_dir}/EFI/BOOT/bootx64.efi" "${inst_dir}/EFI/BOOT/bootx64.efi"
populate_esp "${inst_dir}" "dawOS ${version} (${variant}) - installation medium" \
	"${cmdline_installer}" "${cmdline_installer_verbose}"

# Keep a copy next to the images: it is what the measurement reports cite.
printf '%s\n' "${cmdline}" > "${images_dir}/cmdline.txt"

# --- Disk images ------------------------------------------------------------
sed -e "s/@VERSION@/${version}/g" \
	-e "s/@ROOT_SLOT_SIZE@/${root_slot_size}/g" \
	"${board_dir}/genimage.cfg.in" > "${images_dir}/genimage.cfg"

support/scripts/genimage.sh -c "${images_dir}/genimage.cfg"

echo "post-image: ${images_dir}/disk.img ready (dawOS ${version}, ${variant})"
echo "post-image: ${images_dir}/installer.img ready (installation medium)"
echo "post-image: kernel command line: ${cmdline}"
