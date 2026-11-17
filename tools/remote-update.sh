#!/usr/bin/env bash
# Update a dawOS machine with the last build made here.
#
# Usage: remote-update.sh HOST [--reboot] [--port PORT]
#
# The machine fetches the release itself (dawos-update, systemd-sysupdate),
# checks its signature and writes it to the slot that is not in use. With
# --reboot it then restarts into the new image; "make remote-test" tells
# afterwards which image is running.
#
# How the files travel. The release directory (output/release) is served
# by a web server that listens on this computer's loopback interface only,
# and the SSH connection carries a tunnel from the machine back to it: the
# machine asks its own 127.0.0.1, and the request comes out here. Nothing
# is exposed on the network, no firewall rule is needed, and it works
# through the address translation of WSL2, which a server reachable from
# the machine would not without further setup.
#
# The tunnel is transport, not trust: the machine accepts the release only
# because its list of checksums is signed by the update key.

set -euo pipefail

host=
reboot=
ssh_port=22
while (($# > 0)); do
	case $1 in
	--reboot) reboot=--reboot ;;
	--port)
		ssh_port=${2:?--port needs a port}
		shift
		;;
	-*)
		echo "usage: ${0##*/} HOST [--reboot] [--port PORT]" >&2
		exit 1
		;;
	*) host=$1 ;;
	esac
	shift
done
[[ -n ${host} ]] || {
	echo "usage: ${0##*/} HOST [--reboot] [--port PORT]" >&2
	exit 1
}

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
release=${repo_dir}/output/release
[[ -f ${release}/SHA256SUMS.gpg ]] || {
	echo "remote-update: no release in ${release}: run 'make image' first" >&2
	exit 1
}
version=$(tr -d '[:space:]' < "${repo_dir}/output/images/version.txt")

# Any free port will do, here and on the machine: it only names the two
# ends of the tunnel.
port=8643
python3 -m http.server "${port}" --bind 127.0.0.1 --directory "${release}" > /dev/null 2>&1 &
server_pid=$!
trap 'kill "${server_pid}" 2> /dev/null || true' EXIT
# Give the server a moment to listen.
sleep 1
kill -0 "${server_pid}" 2> /dev/null || {
	echo "remote-update: could not start the local web server on port ${port}" >&2
	exit 1
}

echo "remote-update: offering release ${version} to ${host}"
# ExitOnForwardFailure: do not go on if the tunnel cannot be set up.
log=$(mktemp)
trap 'kill "${server_pid}" 2> /dev/null || true; rm -f "${log}"' EXIT
status=0
ssh -p "${ssh_port}" -o ConnectTimeout=10 -o ExitOnForwardFailure=yes \
	-R "127.0.0.1:${port}:127.0.0.1:${port}" "root@${host}" \
	"dawos-update --server http://127.0.0.1:${port}/ ${reboot}" 2>&1 | tee "${log}" || status=$?

# With --reboot the machine may go down before the session has closed in
# good order: ssh then ends with status 255, although everything that
# had to happen did. That is only accepted when the machine itself said
# it was rebooting into the new image.
if [[ ${status} -ne 0 ]]; then
	if [[ ${status} -eq 255 && -n ${reboot} ]] && grep -q 'rebooting into the new image' "${log}"; then
		echo "remote-update: the connection ended with the reboot of ${host}, as expected."
	else
		exit "${status}"
	fi
fi
