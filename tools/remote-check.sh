#!/usr/bin/env bash
# Check a running dawOS machine over SSH: ask it for its boot report and
# compare it with what is expected, exactly like "make qemu-test" does for
# an image in QEMU.
#
# Usage: remote-check.sh HOST [--port PORT] [--expect KEY=VALUE]...
#
# Logs in as root with a key (dev images only, ADR 0005). The first time,
# the host key of the machine is recorded in ~/.ssh/known_hosts; after
# that a different key is refused, as usual with SSH.

set -euo pipefail

host=${1:-}
[[ -n ${host} && ${host} != -* ]] || {
	echo "usage: ${0##*/} HOST [--port PORT] [--expect KEY=VALUE]..." >&2
	exit 1
}
shift

expectations=(failed_units=0 realtime=1)
port=22
while (($# > 0)); do
	case $1 in
	--port)
		port=${2:?--port needs a number}
		shift
		;;
	--expect)
		expectations+=("${2:?--expect needs KEY=VALUE}")
		shift
		;;
	*)
		echo "remote-check: unknown option '$1'" >&2
		exit 1
		;;
	esac
	shift
done

ssh_options=(
	-o BatchMode=yes
	-o ConnectTimeout=10
	-o StrictHostKeyChecking=accept-new
	-p "${port}"
)

if ! report=$(ssh "${ssh_options[@]}" "root@${host}" /usr/libexec/dawos/boot-report); then
	echo "FAIL: cannot run the boot report on ${host} over SSH" >&2
	exit 1
fi

echo "${report}"

failed=0
for expected in "${expectations[@]}"; do
	if [[ " ${report} " != *" ${expected} "* ]]; then
		echo "FAIL: boot report does not contain '${expected}'" >&2
		failed=1
	fi
done
((failed == 0)) || exit 1

echo "PASS: ${host} matches: ${expectations[*]}"
