#!/bin/sh
# live-callers.sh — who asks the live CPUs to run a function of the
# kernel, and why? (step P1.9a.)
#
# Meant to run on the machine under test, from the development PC, in a
# terminal of its own, started before what it watches and left to end
# after it:
#
#   ssh "root@${H}" 'WATCH_SECONDS=900 sh -s -- LABEL' < tools/live-callers.sh
#
# The engine sees, on its own CPUs, the functions another CPU asked them
# to run in the middle of a compute phase: do_sync_core (the kernel
# rewrote some of its own code), __bpf_arch_ibpb (a classic BPF program
# was loaded, and every CPU forgets its branch predictions),
# flush_tlb_func (memory of a process running there was unmapped). It
# sees them arrive. It cannot see who sent them: that happens on another
# CPU, in another process.
#
# The kernel has an event for the sending side, csd:csd_queue_cpu: one
# CPU puts a function in the queue of another. This script turns it on
# for the live CPUs only, in a trace buffer of its own, with the stack of
# the kernel at each one, for WATCH_SECONDS seconds (default 600). Then
# it says, for each function: how many times, which program asked, and
# from where in the kernel.
#
# The live CPUs are the isolated ones (/sys/devices/system/cpu/isolated);
# LIVE names others, as in LIVE="2 3", to try the script on a machine
# that has none.
#
# Turning an event on and off makes the kernel rewrite its code: this
# script causes a few do_sync_core itself, at its start and at its end.
# What it adds in between is on the CPUs that send, never on the live
# ones.
#
# The script goes on to its end if the connection it was started from
# drops, and writes its report to the disk before showing it: an hour
# with nothing to say is long enough for a connection to be closed on
# the way (it happened twice in step P1.9a, and once the report was
# lost). It also says every five minutes that it is still there.
#
# Results go to /var/lib/dawos/measure/callers-<time>[-LABEL].

set -u

label=${1:-}
WATCH_SECONDS=${WATCH_SECONDS:-600}
LIVE=${LIVE:-}
BASE=/var/lib/dawos/measure
TRACING=/sys/kernel/tracing
INSTANCE="${TRACING}/instances/dawos-callers"

die() {
	echo "live-callers: $*" >&2
	exit 1
}

[ "$(id -u)" = 0 ] || die "must run as root"
case "${WATCH_SECONDS}" in
'' | *[!0-9]*) die "WATCH_SECONDS=${WATCH_SECONDS}: a number of seconds" ;;
esac
[ -d "${TRACING}/events" ] || mount -t tracefs tracefs "${TRACING}" 2> /dev/null
[ -d "${TRACING}/events/csd/csd_queue_cpu" ] ||
	die "this kernel has no event csd:csd_queue_cpu"

# ---- The live CPUs, one by one.
if [ -z "${LIVE}" ]; then
	isolated=$(cat /sys/devices/system/cpu/isolated 2> /dev/null)
	[ -n "${isolated}" ] || die "no isolated CPU on this machine: name the CPUs to watch with LIVE=\"N N\""
	LIVE=$(echo "${isolated}" | tr ',' '\n' | awk -F- '{ for (cpu = $1; cpu <= (NF > 1 ? $2 : $1); cpu++) printf "%d ", cpu }')
fi
filter=
first=
for cpu in ${LIVE}; do
	case "${cpu}" in
	'' | *[!0-9]*) die "LIVE: '${cpu}' is not the number of a CPU" ;;
	esac
	filter="${filter:+${filter} || }cpu == ${cpu}"
	first=${first:-${cpu}}
done

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dir="${BASE}/callers-${stamp}${label:+-${label}}"
mkdir -p "${dir}" || die "cannot create ${dir}"

rmdir "${INSTANCE}" 2> /dev/null
mkdir "${INSTANCE}" || die "cannot make a trace buffer of my own (${INSTANCE})"
event="${INSTANCE}/events/csd/csd_queue_cpu"
finish() {
	echo 0 > "${event}/enable" 2> /dev/null
	echo '!stacktrace' > "${event}/trigger" 2> /dev/null
	rmdir "${INSTANCE}" 2> /dev/null
}
trap finish EXIT
trap 'exit 1' INT TERM
# A connection that drops must not end the watch, nor the report.
trap '' HUP PIPE

echo 2048 > "${INSTANCE}/buffer_size_kb"
echo "${filter}" > "${event}/filter" || die "the kernel refused the filter '${filter}'"
echo "stacktrace if ${filter}" > "${event}/trigger" || die "the kernel refused the trigger"
{
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "label: ${label:-none}"
	echo "kernel: $(uname -a | cut -d' ' -f1,3-)"
	# shellcheck source=/dev/null
	echo "image: $(. /etc/os-release && echo "${IMAGE_VERSION:-unknown}")"
	echo "live_cpus: ${LIVE}"
	echo "watch_seconds: ${WATCH_SECONDS}"
	echo "uptime_s: $(cut -d. -f1 /proc/uptime)"
	if [ -r /var/lib/dawos/campaign.sh ]; then
		echo "campaign: $(sh /var/lib/dawos/campaign.sh --status 2> /dev/null | head -n 1)"
	fi
} > "${dir}/meta.txt"
echo "directory: ${dir}"
echo "watching the CPUs ${LIVE}for ${WATCH_SECONDS} s: start now what is to be watched"

echo 1 > "${event}/enable"
echo 1 > "${INSTANCE}/tracing_on"
left=${WATCH_SECONDS}
while [ "${left}" -gt 0 ]; do
	step=300
	[ "${left}" -lt "${step}" ] && step=${left}
	sleep "${step}"
	left=$((left - step))
	[ "${left}" -gt 0 ] && echo "still watching: ${left} s to go" 2> /dev/null
done
echo 0 > "${INSTANCE}/tracing_on"
cat "${INSTANCE}/trace" > "${dir}/trace.txt"
lost=$(cat "${INSTANCE}"/per_cpu/cpu*/stats 2> /dev/null | awk '/^(overrun|dropped events):/ { total += $NF } END { print total + 0 }')

# ---- What was seen. A line of the event:
#   PROGRAM-PID [CPU] flags TIME: csd_queue_cpu: cpu=N callsite=... func=F csd=...
# then "<stack trace>" and the frames, each " => function".
# A function asked of several live CPUs at once gives one line for each:
# counted once, on the first of them.
{
	echo "watched for ${WATCH_SECONDS} s; events lost by the trace buffer: ${lost}"
	awk -v first="${first}" '
		function program(text) {
			sub(/^ +/, "", text)
			sub(/-[0-9]+$/, "", text)
			return text
		}
		# The stack being read is over: it is counted under its key,
		# and kept if the key has not had one like it (four at most).
		function close_stack(    n, i) {
			if (!taking) return
			taking = 0
			if (text == "") return
			n = stacks[current]
			for (i = 1; i <= n; i++) {
				if (kept[current, i] == text) { seen[current, i]++; return }
			}
			if (n >= 4) { others[current]++; return }
			stacks[current] = ++n
			kept[current, n] = text
			seen[current, n] = 1
		}
		/: csd_queue_cpu: / {
			close_stack()
			split($0, half, ": csd_queue_cpu: ")
			target = half[2]
			sub(/^cpu=/, "", target)
			sub(/ .*/, "", target)
			name = half[2]
			sub(/.* func=/, "", name)
			sub(/ .*/, "", name)
			sub(/\+0x.*/, "", name)
			every[name]++
			if (target != first) next
			# The program and the time, from the left half:
			# "   PROGRAM-PID    [CPU] flags TIME"
			left = half[1]
			time = left
			sub(/.* /, "", time)
			thread = left
			sub(/ +\[[0-9]+\].*/, "", thread)
			sub(/^ +/, "", thread)
			who = program(thread)
			key = name " | " who
			if (!(key in times)) { order[++keys] = key; from[key] = time }
			times[key]++
			till[key] = time
			# The stack comes on lines of its own, after other events
			# when several CPUs send at once: it is found again by
			# the thread that wrote it.
			waiting[thread] = key
			next
		}
		/<stack trace>/ {
			close_stack()
			thread = $0
			sub(/ +\[[0-9]+\].*/, "", thread)
			sub(/^ +/, "", thread)
			if (thread in waiting) {
				current = waiting[thread]
				delete waiting[thread]
				taking = 1
				frames = 0
				text = ""
			}
			next
		}
		taking && / => / {
			frame = $0
			sub(/.* => /, "", frame)
			# The frames of the event itself say nothing.
			if (frame ~ /^(trace_event_raw_event_|trace_event_buffer_|__traceiter_|trace_csd_queue_cpu|__trace_stack|ftrace_trace_stack|event_triggers_)/) next
			if (frames++ < 16) text = text "\n      " frame
			next
		}
		{ close_stack() }
		END {
			close_stack()
			if (keys == 0) { print "nothing was asked of the live CPUs"; exit }
			print "functions asked of the live CPUs, and the program that asked (times; seconds since the boot of the first and of the last):"
			for (i = 1; i <= keys; i++) {
				key = order[i]
				printf "  %s: %d times, from %s to %s\n", key, times[key], from[key], till[key]
			}
			print ""
			print "in all, on every live CPU together:"
			for (name in every) printf "  %s: %d\n", name, every[name]
			print ""
			print "where in the kernel they came from (each way a program took, at most four, with its times):"
			for (i = 1; i <= keys; i++) {
				key = order[i]
				if (stacks[key] == 0) { printf "  %s\n      (no stack was kept)\n", key; continue }
				for (n = 1; n <= stacks[key]; n++) printf "  %s, %d times:%s\n", key, seen[key, n], kept[key, n]
				if (others[key] > 0) printf "  %s, %d times: other ways\n", key, others[key]
			}
		}' "${dir}/trace.txt"
	echo ""
	echo "finished_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "${dir}/summary.txt"
cat "${dir}/summary.txt" 2> /dev/null
