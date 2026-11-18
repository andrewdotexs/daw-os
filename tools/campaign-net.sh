#!/usr/bin/env bash
# Network traffic for the measurement campaign of step P0.7.
#
# Usage: campaign-net.sh HOST [MIB]
#
# It runs on the development PC while tools/campaign.sh runs on the
# machine. The frozen image has no iperf3, so the traffic is made with
# what both ends have: two SSH streams at a time, one in each direction,
# of MIB mebibytes of zeros each (default 4096), round after round. On
# the machine the packets arrive on the CPUs of the io domain and sshd
# works on those of the system domain: never on a live CPU.
#
# It stops by itself when the campaign ends. If the machine cannot be
# reached it says so, with the time, and tries again: a link that went
# down is something the campaign wants to know about (open point A-36).
# The machine counts what really passed: this script only reports what
# it saw from its side, and never prints the address.

set -uo pipefail

host=${1:-}
mib=${2:-4096}
if [[ -z ${host} || ${host} == -* || ! ${mib} =~ ^[0-9]+$ ]]; then
	echo "usage: ${0##*/} HOST [MIB]" >&2
	exit 1
fi

ssh_options=(-o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=3
	-o Compression=no -o BatchMode=yes)

now() {
	date +%H:%M:%S
}

echo "$(now) traffic for the campaign: rounds of ${mib} MiB in each direction; stop with Ctrl-C"
round=0
unreachable=0
while :; do
	status=0
	ssh "${ssh_options[@]}" "root@${host}" systemctl is-active --quiet dawos-campaign || status=$?
	if [[ ${status} -eq 255 ]]; then
		unreachable=$((unreachable + 1))
		echo "$(now) the machine cannot be reached (${unreachable} times so far): trying again in 10 s"
		if [[ ${round} -eq 0 && ${unreachable} -eq 1 ]]; then
			echo "         (if it never answers: 'ssh root@HOST true' must work without asking for anything)"
		fi
		sleep 10
		continue
	fi
	if [[ ${status} -ne 0 ]]; then
		echo "$(now) no campaign is running on the machine: done after ${round} rounds"
		break
	fi

	round=$((round + 1))
	started=$(date +%s.%N)
	dd if=/dev/zero bs=1M count="${mib}" status=none |
		ssh "${ssh_options[@]}" "root@${host}" 'cat > /dev/null' &
	up=$!
	# shellcheck disable=SC2029 # the count is meant to expand here
	ssh "${ssh_options[@]}" "root@${host}" "dd if=/dev/zero bs=1M count=${mib} 2> /dev/null" > /dev/null &
	down=$!
	failed=
	wait "${up}" || failed="to the machine"
	wait "${down}" || failed="${failed:+${failed} and }from the machine"
	elapsed=$(awk -v start="${started}" -v end="$(date +%s.%N)" 'BEGIN { printf "%.1f", end - start }')
	if [[ -n ${failed} ]]; then
		echo "$(now) round ${round}: the stream ${failed} broke after ${elapsed} s"
		sleep 5
	else
		echo "$(now) round ${round}: ${mib} MiB each way in ${elapsed} s, $(awk -v mib="${mib}" -v s="${elapsed}" 'BEGIN { printf "%.0f", mib / s }') MiB/s per direction"
	fi
done
