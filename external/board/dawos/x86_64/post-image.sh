#!/usr/bin/env bash
# dawOS post-image hook: runs after the filesystem images are created.
#
# Called by Buildroot as:  post-image.sh <BINARIES_DIR> <variant>
# with the Buildroot top directory as the working directory.
#
# Step P0.2 scope: assemble a GPT disk image that boots in QEMU through
# systemd-boot. There is no verity, no UKI and no signature yet; those
# arrive in step P0.5 and replace the "boot entry" section below.

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

# --- Kernel command line ----------------------------------------------------
# The root partition is named by its GPT label, which carries the version:
# the same command line works whichever slot the image is written to.
cmdline="root=PARTLABEL=${root_label} $(read_cmdline \
	"${board_dir}/cmdline/common.txt" \
	"${board_dir}/cmdline/profile-generic.txt")"

# --- Boot entry (Boot Loader Specification, type 1) ---------------------------
esp_dir=${images_dir}/efi-part

# Drop Buildroot's sample configuration and anything left by an earlier run.
rm -rf "${esp_dir}/loader" "${esp_dir}/dawos"

install -D -m 0644 "${images_dir}/bzImage" "${esp_dir}/dawos/${version}/linux"

install -d -m 0755 "${esp_dir}/loader/entries"
cat > "${esp_dir}/loader/loader.conf" <<LOADER
# dawOS (${variant}): show the menu briefly so a slot can be picked by hand.
timeout 2
default dawos-*
console-mode keep
LOADER

cat > "${esp_dir}/loader/entries/dawos-${version}.conf" <<ENTRY
title   dawOS ${version} (${variant})
version ${version}
sort-key dawos
linux   /dawos/${version}/linux
options ${cmdline}
ENTRY

# Keep a copy next to the images: it is what the measurement reports cite.
printf '%s\n' "${cmdline}" > "${images_dir}/cmdline.txt"

# --- Disk image -------------------------------------------------------------
sed -e "s/@VERSION@/${version}/g" \
	-e "s/@ROOT_SLOT_SIZE@/${root_slot_size}/g" \
	"${board_dir}/genimage.cfg.in" > "${images_dir}/genimage.cfg"

support/scripts/genimage.sh -c "${images_dir}/genimage.cfg"

echo "post-image: ${images_dir}/disk.img ready (dawOS ${version}, ${variant})"
echo "post-image: kernel command line: ${cmdline}"
