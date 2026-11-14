#!/usr/bin/env bash
# Prepare a dawOS machine for measurements. Run it after every boot.
#
# Usage: prepare-measure.sh [--port PORT] HOST
#
# Until step P0.5 two things are lost at every reboot of the machine, and
# this script puts both back in one SSH session:
#
# 1. The measurement tools newer than the installed image: the current
#    dawos-hwnoise, dawos-thermal-sweep and dawos-topology are copied to
#    /run, from where they are run as "sh /run/<tool>".
# 2. The results: /var is in memory, so /var/lib/dawos/measure is too. The
#    "state" partition already exists on the disk (the installer creates
#    it) but nothing mounts it yet; here it is mounted and its directory
#    lib/dawos/measure is put in place of the one in memory. That is the
#    path the results will have when step P0.5 mounts "state" as /var, so
#    they will still be where the tools expect them.
#
# It is safe to run twice, and it does not touch the results directory
# while a measurement is running.

set -euo pipefail

port=22
host=
while [[ $# -gt 0 ]]; do
	case "$1" in
	--port)
		port=${2:?--port needs a port}
		shift 2
		;;
	-*)
		echo "usage: ${0##*/} [--port PORT] HOST" >&2
		exit 1
		;;
	*)
		host=$1
		shift
		;;
	esac
done
[[ -n ${host} ]] || {
	echo "usage: ${0##*/} [--port PORT] HOST" >&2
	exit 1
}

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tools_dir=${repo_dir}/external/board/dawos/x86_64/rootfs-overlay/usr/bin
tools=(dawos-hwnoise dawos-thermal-sweep dawos-topology)

# Runs on the machine. The tools arrive as a tar archive on standard
# input; everything else is plain POSIX shell for the busybox ash there.
# shellcheck disable=SC2016 # expanded by the remote shell, not here
remote_script='
set -eu
tar -C /run -xf -
echo "Tools copied to /run:"
for tool in /run/dawos-*; do
	[ -f "${tool}" ] && echo "  ${tool}"
done

measure=/var/lib/dawos/measure
state_dev=/dev/disk/by-partlabel/state
state_mnt=/run/dawos-state

if grep -q " ${measure} " /proc/mounts; then
	echo "Results directory: already on the state partition."
elif systemctl is-active --quiet dawos-hwnoise || systemctl is-active --quiet dawos-thermal-sweep; then
	echo "Results directory: left in memory, a measurement is running. Run this again when it has finished."
elif [ ! -b "${state_dev}" ]; then
	echo "Results directory: IN MEMORY (no state partition on this machine). Fetch the results before rebooting."
else
	mkdir -p "${state_mnt}" "${measure}"
	grep -q " ${state_mnt} " /proc/mounts || mount -t ext4 "${state_dev}" "${state_mnt}"
	mkdir -p "${state_mnt}/lib/dawos/measure"
	# Results already in memory move to the disk with the directory.
	cp -a "${measure}/." "${state_mnt}/lib/dawos/measure/"
	mount --bind "${state_mnt}/lib/dawos/measure" "${measure}"
	echo "Results directory: now on the state partition, it survives a reboot."
fi

echo "Results on the machine:"
found=no
for dir in "${measure}"/*/; do
	[ -d "${dir}" ] || continue
	found=yes
	echo "  $(basename "${dir}")"
done
[ "${found}" = yes ] || echo "  (none)"
'

tar -C "${tools_dir}" -cf - "${tools[@]}" |
	ssh -p "${port}" -o ConnectTimeout=10 "root@${host}" "${remote_script}"
