#!/bin/sh
# campaign.sh — the measurement campaign of step P0.7, on a dawOS machine.
#
# It runs ON the machine, not on the development PC, and only with the
# tools the installed image already has: the configuration under test is
# frozen, so nothing new may be installed for the campaign. "make
# campaign" copies this file to /var/lib/dawos and runs it there.
#
# What it measures: how late a thread of priority 95 wakes up on the
# live CPUs while every other domain of the machine works. A campaign is
# a plan, a list of steps run one after the other:
#
#   timerlat:SECONDS    "rtla timerlat hist" with threads in user space;
#                       above the figure aimed at a trace is saved and
#                       the measurement goes on;
#   cyclictest:SECONDS  the same question asked to a second, independent
#                       tool.
#
# A step runs only if the one before it passed: delay within the
# limit, no system management interrupt, no thermal throttling, no
# device interrupt on a live CPU.
#
# The load never touches the live CPUs and comes in cycles of phases,
# because the worst delays hide in the changes, not in the steady state:
#
#   full      the load of document 0: stress-ng (computation, memory,
#             context switches, timers) on every CPU of the preemptive
#             and system domains, fio on the CPUs of the io domain. On
#             the 285K the package settles at 120 W;
#   power     computation alone on all the CPUs that are not live, no
#             fio: the package at its power limit, 160 W on the 285K
#             (open point A-30);
#   burst-a   short bursts on the P-cores that are not live: busy 1 ms
#             every 5 (open point A-33), fio as above;
#   burst-b   the same, busy 5 ms every 17;
#   burst-c   timers and context switches at a few kHz on those P-cores;
#   rest      nothing at all.
#
# At the end of every phase the largest delay seen in it is read from
# the tracer and written to phases.txt: reading it costs the live CPUs
# nothing.
#
# Next to the measurement, and never on a live CPU: turbostat every 5
# seconds on the other CPUs (SMI count, power, temperature); once a
# minute the thermal-throttle counters, the state of the network link
# and its traffic, the device interrupts of the live CPUs. The SMI
# counter of the live CPUs themselves is read before and after each
# step, because reading it interrupts the CPU.
#
# Network traffic comes from another machine: tools/campaign-net.sh.
#
# Results: /var/lib/dawos/measure/campaign-<time>[-label]/.

set -u

PROG=campaign.sh
BASE=/var/lib/dawos/measure
UNIT=dawos-campaign
RUN=/run/dawos-campaign
TOPOLOGY=/run/dawos/topology.json
TRACING=/sys/kernel/tracing

# Acceptance of the phase (document 0): 50 us is the limit, 20 us the
# figure aimed at.
THRESHOLD_US=50
TARGET_US=20
PRIORITY=95
# The period of timerlat is the one of document 0. cyclictest keeps the
# millisecond of every measurement of step P0.6, so that the two tools
# also cover the two periods.
PERIOD_US=250
CYCLICTEST_PERIOD_US=1000
# Histogram of timerlat: 250 ns per bucket, up to 100 us.
BUCKET_NS=250
BUCKETS=400
# Reference measurements start at least five minutes after power-on
# (open point A-17).
MIN_UPTIME_S=300
# A trace of the kernel is saved for every delay above the figure aimed
# at: it shows what ran on the live CPU at that moment. Each one costs a
# few seconds in which rtla does not measure, so after the first
# MAX_TRACES the bar goes up to the limit. Of each trace the last lines
# are kept: the cause is there.
TRACE_US=${TARGET_US}
MAX_TRACES=20
TRACE_LINES=1500
PAUSE_S=30

PLAN_SHORT="timerlat:600 timerlat:3600 cyclictest:3600"
PLAN_LONG="timerlat:86400"
# Five minutes to see that every piece works on the machine at hand.
PLAN_TRIAL="timerlat:200 cyclictest:60"
CYCLE_TRIAL="full:45 power:45 burst-a:20 burst-b:20 burst-c:20 rest:30"
# Cycles of ten minutes up to a few hours, of half an hour beyond: long
# enough at full power for the temperature to settle.
CYCLE_SHORT="full:420 burst-a:40 burst-b:40 burst-c:40 rest:60"
CYCLE_LONG="full:780 power:600 burst-a:100 burst-b:100 burst-c:100 rest:120"
CYCLE_LONG_FROM_S=21600

usage() {
	cat <<USAGE
Usage:
  ${PROG} --start PLAN [options]   start a campaign in the background
  ${PROG} --status                 what is running, or the last summary
  ${PROG} --stop                   stop the campaign that is running
  ${PROG} --summary DIR            rewrite and show the summary of DIR
  ${PROG} --cleanup                remove the load file from "shared"

PLAN is "short" (${PLAN_SHORT}),
"long" (${PLAN_LONG}), "trial" (${PLAN_TRIAL},
with short phases: it only shows that every piece works) or a list of
steps of its own: "timerlat:60 cyclictest:60".

Options of --start:
  --label NAME        added to the name of the result directory
  --write-mbps N      what fio writes, in MiB/s (default 25; 0: no limit)
  --file-mb N         size of the file fio works on (default 65536)
  --no-disk           no fio at all
  --shared-dev DEV    where the file goes (default: the partition "shared")
  --cycle "PHASES"    phases of a cycle, as name:seconds
  --vm-bytes SIZE     memory of each of the four stress-ng vm workers (default 2G)
  --trace-us N        save a trace above N us (default ${TRACE_US}; the first ${MAX_TRACES})
  --threshold-us N    for trials only: the limit, instead of ${THRESHOLD_US} us
  --any-uptime        do not wait for the fifth minute after power-on
USAGE
}

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

# CPUs of a kernel list ("0-3,8-15"), one per line, sorted.
expand_list() {
	echo "$1" | tr ',' '\n' | while IFS=- read -r first last; do
		case "${first}" in
		'' | *[!0-9]*) continue ;;
		esac
		seq "${first}" "${last:-${first}}"
	done | sort -n -u
}

# The CPUs given one per line on standard input as a list: "0,1,2,3".
join_list() {
	tr '\n' ',' | sed 's/,$//'
}

uptime_now() {
	read -r uptime_value _ < /proc/uptime
	echo "${uptime_value}"
}

uptime_s() {
	read -r uptime_value _ < /proc/uptime
	echo "${uptime_value%%.*}"
}

# The value of a "key: value" line of the file $1.
field() {
	sed -n "s/^$2: //p" "$1" | head -n 1
}

# The CPUs of a domain, from the report of dawos-tune.
domain() {
	sed -n "s/^ *\"$1\": \"\([0-9,-]*\)\".*/\1/p" "${TOPOLOGY}" 2> /dev/null | head -n 1
}

# "core=N package=M": thermal-throttle events since boot; nothing where
# the kernel does not count them (virtual machines).
throttle_counts() {
	core=0
	package=0
	found=0
	for file in /sys/devices/system/cpu/cpu[0-9]*/thermal_throttle/core_throttle_count; do
		[ -r "${file}" ] || continue
		found=1
		read -r count < "${file}"
		core=$((core + count))
		read -r count < "${file%core_throttle_count}package_throttle_count"
		[ "${count}" -gt "${package}" ] && package=${count}
	done
	[ "${found}" = 1 ] && echo "core=${core} package=${package}"
	return 0
}

# "cpu count" for every CPU: system management interrupts since boot
# (MSR 0x34). Reading it interrupts the CPU: never during a step.
smi_counts() {
	for msr in /dev/cpu/[0-9]*/msr; do
		[ -r "${msr}" ] || continue
		cpu=${msr#/dev/cpu/}
		cpu=${cpu%/msr}
		count=$(dd if="${msr}" bs=8 skip=52 count=1 iflag=skip_bytes 2> /dev/null | od -An -tu8 | tr -d ' \n')
		[ -n "${count}" ] && echo "${cpu} ${count}"
	done | sort -n
}

# The network interface of the machine: the first real one that is up.
network_interface() {
	for path in /sys/class/net/*; do
		[ -e "${path}/device" ] || continue
		read -r state < "${path}/operstate"
		[ "${state}" = up ] || continue
		echo "${path##*/}"
		return 0
	done
	return 0
}

# "changes rx_bytes tx_bytes" of the interface $1; "- - -" without one.
network_counts() {
	if [ -n "$1" ] && [ -r "/sys/class/net/$1/carrier_changes" ]; then
		echo "$(cat "/sys/class/net/$1/carrier_changes") $(cat "/sys/class/net/$1/statistics/rx_bytes") $(cat "/sys/class/net/$1/statistics/tx_bytes")"
	else
		echo "- - -"
	fi
}

# What reached the CPUs $2 according to the copy $1 of /proc/interrupts:
# "name count description", one line per interrupt. Device interrupts
# have a number for a name, those of the kernel itself letters.
interrupts_of() {
	awk -v cpus="$(expand_list "$2" | tr '\n' ' ')" '
		BEGIN { wanted = split(cpus, cpu, " ") }
		NR == 1 { columns = NF; next }
		{
			name = $1
			sub(/:$/, "", name)
			sum = 0
			for (i = 1; i <= wanted; i++) {
				column = cpu[i] + 2
				if (column <= NF && $column ~ /^[0-9]+$/) sum += $column
			}
			text = ""
			for (i = columns + 2; i <= NF; i++) text = text (text == "" ? "" : " ") $i
			print name, sum, text
		}' "$1"
}

# The interrupts that grew between the copies $1 and $2 of
# /proc/interrupts, on the CPUs $3: "name growth description".
interrupts_grown() {
	interrupts_of "$1" "$3" > "${RUN}/irq-before.txt"
	interrupts_of "$2" "$3" | awk -v before="${RUN}/irq-before.txt" '
		BEGIN { while ((getline line < before) > 0) { split(line, part, " "); old[part[1]] = part[2] } }
		{
			growth = $2 - old[$1]
			if (growth > 0) { $2 = growth; print }
		}'
}

# The device interrupts of the live CPUs since boot, as one number.
live_device_interrupts() {
	cat /proc/interrupts > "${RUN}/irq-now.txt"
	interrupts_of "${RUN}/irq-now.txt" "$1" | awk '$1 ~ /^[0-9]+$/ { sum += $2 } END { print sum + 0 }'
}

# Latency figures of a histogram of "rtla timerlat hist" (file $1), as
# "key: value" lines, in microseconds. rtla prints a header naming the
# columns (IRQ-004 Thr-004 Usr-004 ...), one row per bucket that is not
# empty, then the rows over:, count:, min:, avg:, max: with the same
# columns, then a block for all CPUs together, which starts with "ALL:"
# and is not read. The percentile is the upper edge of its bucket.
timerlat_fields() {
	awk -v bucket="${BUCKET_NS}" -v buckets="${BUCKETS}" '
		function percentile(kind,    target, sum, i) {
			target = total[kind] * 0.99999
			sum = 0
			for (i = 1; i <= rows; i++) {
				sum += hist[kind, row[i]]
				if (sum >= target) return sprintf("%.2f", (row[i] + bucket) / 1000)
			}
			return sprintf(">%d", bucket * buckets / 1000)
		}
		function show(value) { return sprintf("%.2f", value / 1000) }
		/^ALL:/ { done = 1 }
		done { next }
		$1 == "Index" {
			for (i = 2; i <= NF; i++) kind[i] = substr($i, 1, 3)
			columns = NF
			next
		}
		!columns { next }
		$1 ~ /^[0-9]+$/ {
			row[++rows] = $1
			for (i = 2; i <= NF; i++) hist[kind[i], $1] += $i
			next
		}
		$1 == "over:" {
			for (i = 2; i <= NF; i++) over[kind[i]] += $i
			next
		}
		$1 == "count:" {
			for (i = 2; i <= NF; i++) { count[i] = $i + 0; total[kind[i]] += $i }
			next
		}
		$1 == "avg:" {
			for (i = 2; i <= NF; i++) if ($i != "-") weighted[kind[i]] += $i * count[i]
			next
		}
		$1 == "max:" {
			for (i = 2; i <= NF; i++) if ($i != "-" && $i + 0 > high[kind[i]]) high[kind[i]] = $i + 0
			next
		}
		END {
			if (!total["Usr"]) { print "samples: 0"; exit }
			printf "samples: %d\n", total["Usr"]
			printf "irq_max_us: %s\n", show(high["IRQ"])
			printf "thr_max_us: %s\n", show(high["Thr"])
			printf "usr_avg_us: %s\n", show(weighted["Usr"] / total["Usr"])
			printf "usr_p99999_us: %s\n", percentile("Usr")
			printf "usr_max_us: %s\n", show(high["Usr"])
			printf "beyond_histogram: %d\n", over["Usr"]
		}' "$1"
}

# The same figures from the output of "cyclictest -h" (file $1): one row
# per microsecond with a column per thread, then lines that start with
# "# Avg Latencies:", "# Max Latencies:" and "# Histogram Overflows:",
# the samples beyond the last row.
cyclictest_fields() {
	awk '
		$1 ~ /^[0-9]+$/ {
			row[++rows] = $1 + 0
			for (i = 2; i <= NF; i++) hist[$1 + 0] += $i
			next
		}
		/^# Avg Latencies:/ { for (i = 4; i <= NF; i++) { average += $i; threads++ } }
		/^# Max Latencies:/ { for (i = 4; i <= NF; i++) if ($i + 0 > high) high = $i + 0 }
		/^# Histogram Overflows:/ { for (i = 4; i <= NF; i++) over += $i }
		END {
			total = over
			for (i = 1; i <= rows; i++) total += hist[row[i]]
			if (!total) { print "samples: 0"; exit }
			printf "samples: %d\n", total
			target = total * 0.99999
			found = ""
			for (i = 1; i <= rows; i++) {
				sum += hist[row[i]]
				if (sum >= target) { found = row[i] + 1; break }
			}
			printf "usr_avg_us: %.2f\n", threads ? average / threads : 0
			printf "usr_p99999_us: %s\n", found == "" ? ">100" : sprintf("%.2f", found)
			printf "usr_max_us: %.2f\n", high
			printf "beyond_histogram: %d\n", over
		}' "$1"
}

# Power and temperature from the readings of turbostat (file $1), whose
# first line names the columns: "watt_avg watt_max temperature_max
# smi readings"; "-" for what the machine does not report.
turbostat_fields() {
	awk '
		NR == 1 || $1 == "Time_Of_Day_Seconds" {
			for (i = 1; i <= NF; i++) column[$i] = i
			next
		}
		{
			readings++
			if ("PkgWatt" in column) {
				watt += $column["PkgWatt"]
				if ($column["PkgWatt"] + 0 > top) top = $column["PkgWatt"] + 0
			}
			if ("PkgTmp" in column && $column["PkgTmp"] + 0 > hot) hot = $column["PkgTmp"] + 0
			if ("SMI" in column) smi += $column["SMI"]
		}
		END {
			if (!readings) { print "- - - - 0"; exit }
			printf "%s %s %s %s %d\n",
				("PkgWatt" in column) ? sprintf("%.1f", watt / readings) : "-",
				("PkgWatt" in column) ? sprintf("%.1f", top) : "-",
				("PkgTmp" in column) ? hot : "-",
				("SMI" in column) ? smi : "-", readings
		}' "$1" 2> /dev/null
}

# The trace instances of rtla, which stay behind when it is killed: a
# tracer left running would go on working on the live CPUs.
remove_instances() {
	pgrep -x rtla > /dev/null && return 0
	for name in timerlat_hist osnoise_trace; do
		instance=${TRACING}/instances/${name}
		[ -d "${instance}" ] || continue
		echo 0 > "${instance}/tracing_on" 2> /dev/null
		echo nop > "${instance}/current_tracer" 2> /dev/null
		rmdir "${instance}" 2> /dev/null
	done
	return 0
}

# --- load ------------------------------------------------------------------

# Every process started for the load or next to the measurement, to be
# stopped whatever happens.
children=

remember() {
	children="${children} $1"
}

stop_children() {
	for pid in ${children}; do
		kill "${pid}" 2> /dev/null
	done
	for pid in ${children}; do
		wait "${pid}" 2> /dev/null
	done
	children=
	# stress-ng and fio start workers of their own.
	pkill -x stress-ng 2> /dev/null
	pkill -x fio 2> /dev/null
	return 0
}

# fio on the CPUs of the io domain for $1 seconds: two jobs read at
# random without limit, one writes at the rate asked for and flushes
# every 16 MiB, as a recording does. One line of figures goes to $2.
start_fio() {
	[ "${disk}" = yes ] || return 0
	rate=
	[ "${write_mbps}" -gt 0 ] && rate="--rate=${write_mbps}m"
	(
		# shellcheck disable=SC2086 # rate is empty or one option
		taskset -c "${io_cpus}" fio --output-format=terse \
			--name=global --filename="${load_file}" --ioengine=io_uring --direct=1 \
			--time_based --runtime="$1" \
			--name=read --rw=randread --bs=64k --iodepth=16 --numjobs=2 \
			--name=write --rw=randwrite --bs=1M --iodepth=4 --fdatasync=16 ${rate} \
			2> "${RUN}/fio.err" |
			awk -F ';' -v phase="$3" '
				$3 == "read" { read += $7 }
				$3 == "write" { write += $48 }
				END { printf "%s read_mib_s=%.0f write_mib_s=%.1f\n", phase, read / 1024, write / 1024 }' >> "$2"
	) &
	fio_pid=$!
	remember "${fio_pid}"
}

# One phase of a cycle: $1 name, $2 seconds, $3 directory of the step,
# $4 number of the cycle.
run_phase() {
	phase=$1
	length=$2
	phase_dir=$3
	max_file=${TRACING}/instances/timerlat_hist/tracing_max_latency
	[ -w "${max_file}" ] && echo 0 > "${max_file}"
	phase_start=$(date +%s)
	stress_pid=
	fio_pid=
	case "${phase}" in
	full)
		start_fio "${length}" "${phase_dir}/fio.txt" "$4-${phase}"
		stress-ng --taskset "${other_cpus}" --temp-path "${RUN}" \
			--cpu "${cpu_workers}" --cpu-method matrixprod \
			--vm 4 --vm-bytes "${vm_bytes}" --switch 8 --timer 4 \
			--timeout "${length}s" > "${RUN}/stress.txt" 2>&1 &
		stress_pid=$!
		;;
	power)
		stress-ng --taskset "${quiet_cpus}" --temp-path "${RUN}" \
			--cpu "${power_workers}" --cpu-method matrixprod \
			--timeout "${length}s" > "${RUN}/stress.txt" 2>&1 &
		stress_pid=$!
		;;
	burst-a | burst-b)
		busy=20
		slice=1
		if [ "${phase}" = burst-b ]; then
			busy=30
			slice=5
		fi
		start_fio "${length}" "${phase_dir}/fio.txt" "$4-${phase}"
		stress-ng --taskset "${burst_cpus}" --temp-path "${RUN}" \
			--cpu "${burst_workers}" --cpu-method matrixprod \
			--cpu-load "${busy}" --cpu-load-slice "${slice}" \
			--timeout "${length}s" > "${RUN}/stress.txt" 2>&1 &
		stress_pid=$!
		;;
	burst-c)
		start_fio "${length}" "${phase_dir}/fio.txt" "$4-${phase}"
		stress-ng --taskset "${burst_cpus}" --temp-path "${RUN}" \
			--timer "${burst_workers}" --timer-freq 4000 \
			--switch "${burst_workers}" --switch-freq 2000 \
			--timeout "${length}s" > "${RUN}/stress.txt" 2>&1 &
		stress_pid=$!
		;;
	*)
		sleep "${length}" &
		stress_pid=$!
		echo "${stress_pid}" > "${RUN}/rest.pid"
		;;
	esac
	remember "${stress_pid}"
	wait "${stress_pid}"
	stress_status=$?
	[ -n "${fio_pid}" ] && wait "${fio_pid}"
	children=
	rm -f "${RUN}/rest.pid"
	if [ "${stress_status}" -ne 0 ] && [ "${phase}" != rest ] && [ ! -e "${RUN}/stop" ]; then
		echo "note: cycle $4 ${phase}: stress-ng ended with status ${stress_status}: $(tail -n 1 "${RUN}/stress.txt" 2> /dev/null)" >> "${phase_dir}/notes.txt"
	fi
	phase_max=-
	if [ -r "${max_file}" ]; then
		read -r phase_max < "${max_file}"
	fi
	echo "$4 ${phase} ${phase_start} $(date +%s) ${phase_max}" >> "${phase_dir}/phases.txt"
}

# Cycles of phases in the directory $1 until the uptime $2.
run_phases() {
	cycle_number=0
	while [ "$(uptime_s)" -lt "$2" ] && [ ! -e "${RUN}/stop" ]; do
		cycle_number=$((cycle_number + 1))
		for item in ${cycle}; do
			[ -e "${RUN}/stop" ] && break
			# A phase of a few seconds at the end of a step tells nothing.
			left=$(($2 - $(uptime_s)))
			[ "${left}" -ge 5 ] || break 2
			length=${item#*:}
			[ "${length}" -gt "${left}" ] && length=${left}
			run_phase "${item%%:*}" "${length}" "$1" "${cycle_number}"
		done
	done
}

# --- one step ---------------------------------------------------------------

# Once a minute, without touching a live CPU: uptime, thermal throttling,
# network link and traffic, device interrupts of the live CPUs.
sample_loop() {
	while :; do
		throttle=$(throttle_counts | tr ' ' ',')
		echo "$(uptime_s) ${throttle:--} $(network_counts "${interface}") $(live_device_interrupts "${live_cpus}")" >> "$1/samples.txt"
		sleep 60
	done
}

# The state of the counters at one end of a step: $1 directory, $2
# "start" or "end".
snapshot() {
	cat /proc/interrupts > "$1/interrupts-$2.txt"
	smi_counts > "$1/smi-$2.txt"
	throttle_counts > "$1/throttle-$2.txt"
	network_counts "${interface}" > "$1/network-$2.txt"
}

# Called by rtla each time a delay is above the bar of the trace: $1
# directory of the step. Counts the event and keeps the end of the
# trace; after MAX_TRACES it raises the bar to the limit of the
# campaign, so that a frequent event cannot blind the measurement.
record_hit() {
	hits=0
	[ -r "$1/trace-count" ] && read -r hits < "$1/trace-count"
	hits=$((hits + 1))
	echo "${hits}" > "$1/trace-count"
	echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $(uptime_now) ${hits}" >> "$1/traces.txt"
	if [ "${hits}" -le "${MAX_TRACES}" ] && [ -r "${RUN}/trace.txt" ]; then
		tail -n "${TRACE_LINES}" "${RUN}/trace.txt" > "$1/trace-${hits}.txt"
	fi
	if [ "${hits}" -eq "${MAX_TRACES}" ]; then
		limit=$(field "$1/../meta.txt" threshold_us)
		if [ -n "${limit}" ] && echo "${limit}" > "${TRACING}/osnoise/stop_tracing_total_us"; then
			echo "note: ${MAX_TRACES} traces saved: from $(date -u +%Y-%m-%dT%H:%M:%SZ) a trace is saved only above ${limit} us" >> "$1/notes.txt"
		else
			echo "note: could not raise the bar of the trace after ${MAX_TRACES} traces" >> "$1/notes.txt"
		fi
	fi
	return 0
}

# One step: $1 its directory, $2 kind, $3 seconds.
run_step() {
	step_dir=$1
	kind=$2
	duration=$3
	mkdir -p "${step_dir}"
	remove_instances
	started=$(date +%s)
	snapshot "${step_dir}" start

	turbostat --quiet --Summary --cpu "${quiet_cpus}" --interval 5 \
		--show Time_Of_Day_Seconds,SMI,Busy%,Bzy_MHz,PkgTmp,PkgWatt \
		--out "${step_dir}/turbostat.txt" 2> "${step_dir}/turbostat.err" &
	turbostat_pid=$!
	sample_loop "${step_dir}" &
	sampler_pid=$!
	monitors="${turbostat_pid} ${sampler_pid}"
	sleep 3

	case "${kind}" in
	timerlat)
		rtla timerlat hist -u -c "${live_cpus}" -H "${system_cpus}" \
			-P "f:${PRIORITY}" -p "${PERIOD_US}" -d "${duration}s" \
			-n -b "${BUCKET_NS}" -E "${BUCKETS}" --no-aa --warm-up 5 \
			-T "${trace_us}" \
			--on-threshold "trace,file=${RUN}/trace.txt" \
			--on-threshold "shell,command=/bin/sh ${SELF} --hit ${step_dir}" \
			--on-threshold continue \
			> "${step_dir}/timerlat.txt" 2>&1 &
		;;
	*)
		threads=$(expand_list "${live_cpus}" | wc -l)
		# --default-system: cyclictest would otherwise forbid every
		# idle state on every CPU, which is not the machine under test.
		cyclictest -q -m -p "${PRIORITY}" -a "${live_cpus}" -t "${threads}" \
			-i "${CYCLICTEST_PERIOD_US}" -D "${duration}" -h 100 --mainaffinity "${system_cpus}" \
			--default-system > "${RUN}/cyclictest.txt" 2>&1 &
		;;
	esac
	tool_pid=$!
	sleep 7
	run_phases "${step_dir}" $(($(uptime_s) + duration - 2))
	wait "${tool_pid}"
	tool_status=$?
	tool_pid=
	# Beyond its histogram cyclictest lists every cycle, one by one.
	[ "${kind}" = timerlat ] || grep -v '^# Thread [0-9]*:' "${RUN}/cyclictest.txt" > "${step_dir}/cyclictest.txt"

	# shellcheck disable=SC2086 # two process numbers
	kill ${monitors} 2> /dev/null
	# shellcheck disable=SC2086
	wait ${monitors} 2> /dev/null
	snapshot "${step_dir}" end

	# What the kernel said meanwhile. Addresses, should a driver print
	# one, must not travel with the results.
	journalctl -k -q --no-pager -o short-monotonic --since "@${started}" 2> /dev/null |
		grep -i -E 'link is|thermal|throttl|mce|machine check|hardware error|stall|hung|blocked for|nvme.*(timeout|reset)|i/o error|bug|oops|warn' |
		sed -E 's/([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}/[address]/g; s/[0-9]{1,3}(\.[0-9]{1,3}){3}/[address]/g' |
		tail -n 200 > "${step_dir}/kernel-log.txt"

	judge "${step_dir}" "${kind}" "${duration}" "${tool_status}"
}

# Write result.txt of the step in $1: the figures and the verdict.
judge() {
	result_dir=$1
	{
		echo "kind: $2"
		echo "seconds: $3"
		echo "tool_exit_status: $4"
		if [ "$2" = timerlat ]; then
			timerlat_fields "${result_dir}/timerlat.txt"
		else
			cyclictest_fields "${result_dir}/cyclictest.txt"
		fi
		hits=0
		[ -r "${result_dir}/trace-count" ] && read -r hits < "${result_dir}/trace-count"
		echo "traces: ${hits}"

		# System management interrupts: the counter of every CPU, read
		# before and after; and what turbostat saw meanwhile.
		if [ -s "${result_dir}/smi-start.txt" ] && [ -s "${result_dir}/smi-end.txt" ]; then
			awk 'NR == FNR { old[$1] = $2; next } { growth += $2 - old[$1] } END { printf "smi: %d\n", growth }' \
				"${result_dir}/smi-start.txt" "${result_dir}/smi-end.txt"
		else
			echo "smi: unknown"
		fi
		if [ -s "${result_dir}/throttle-start.txt" ] && [ -s "${result_dir}/throttle-end.txt" ]; then
			echo "throttle: $(tr ' =' '\n ' < "${result_dir}/throttle-start.txt" > "${RUN}/throttle-before.txt"
				tr ' =' '\n ' < "${result_dir}/throttle-end.txt" |
					awk -v before="${RUN}/throttle-before.txt" '
						BEGIN { while ((getline line < before) > 0) { split(line, part, " "); old[part[1]] = part[2] } }
						{ growth += $2 - old[$1] }
						END { print growth + 0 }')"
		else
			echo "throttle: unknown"
		fi
		interrupts_grown "${result_dir}/interrupts-start.txt" "${result_dir}/interrupts-end.txt" "${live_cpus}" > "${result_dir}/interrupts-live.txt"
		echo "device_interrupts_live: $(awk '$1 ~ /^[0-9]+$/ { sum += $2 } END { print sum + 0 }' "${result_dir}/interrupts-live.txt")"
		read -r changes_before received_before sent_before < "${result_dir}/network-start.txt"
		read -r changes_after received_after sent_after < "${result_dir}/network-end.txt"
		if [ "${changes_before}" != - ]; then
			# The link goes down and comes back: two changes per drop.
			echo "link_changes: $((changes_after - changes_before))"
			echo "network_received_mib: $(((received_after - received_before) / 1048576))"
			echo "network_sent_mib: $(((sent_after - sent_before) / 1048576))"
		else
			echo "link_changes: unknown"
		fi
		# shellcheck disable=SC2046 # five fields
		set -- $(turbostat_fields "${result_dir}/turbostat.txt")
		echo "package_watt_avg: ${1:--}"
		echo "package_watt_max: ${2:--}"
		echo "package_temperature_max_c: ${3:--}"
		echo "smi_seen_by_turbostat: ${4:--}"
		echo "turbostat_readings: ${5:-0}"
	} > "${result_dir}/result.txt"

	# The verdict. What the machine cannot report (a virtual machine has
	# no SMI counter) is said, and does not fail the step by itself.
	awk -v limit="${threshold_us}" -v target="${TARGET_US}" -F ': ' '
		{ value[$1] = $2 }
		END {
			if (value["samples"] + 0 == 0) reason = reason " no samples;"
			if (value["tool_exit_status"] + 0 != 0) reason = reason " the tool ended with status " value["tool_exit_status"] ";"
			if (value["usr_max_us"] + 0 > limit) reason = reason " delay " value["usr_max_us"] " us above " limit " us;"
			if (value["smi"] != "unknown" && value["smi"] + 0 != 0) reason = reason " " value["smi"] " system management interrupts;"
			if (value["smi_seen_by_turbostat"] != "-" && value["smi_seen_by_turbostat"] + 0 != 0) reason = reason " turbostat counted system management interrupts;"
			if (value["throttle"] != "unknown" && value["throttle"] + 0 != 0) reason = reason " " value["throttle"] " thermal-throttle events;"
			if (value["device_interrupts_live"] + 0 != 0) reason = reason " " value["device_interrupts_live"] " device interrupts on live CPUs;"
			if (value["smi"] == "unknown") unknown = unknown " smi"
			if (value["throttle"] == "unknown") unknown = unknown " throttle"
			if (reason == "") printf "verdict: PASS%s\n", unknown == "" ? "" : " (not reported by this machine:" unknown ")"
			else printf "verdict: FAIL:%s\n", reason
			if (value["samples"] + 0 > 0) printf "target_%d_us: %s\n", target, (value["usr_max_us"] + 0 <= target) ? "met" : "not met"
		}' "${result_dir}/result.txt" > "${RUN}/verdict.txt"
	cat "${RUN}/verdict.txt" >> "${result_dir}/result.txt"
}

# --- the campaign -------------------------------------------------------------

# Mount "shared" if nobody did and write the file fio works on, once: a
# file that was only allocated would be read without touching the disk.
prepare_disk() {
	[ "${disk}" = yes ] || return 0
	device=$(readlink -f "${shared_dev}")
	mount_point=$(awk -v device="${device}" '$1 == device { print $2; exit }' /proc/mounts)
	if [ -z "${mount_point}" ]; then
		mount_point=${RUN}/shared
		mkdir -p "${mount_point}"
		if ! mount "${shared_dev}" "${mount_point}"; then
			echo "note: could not mount ${shared_dev}: no disk load" >> "${dir}/notes.txt"
			disk=no
			return 0
		fi
		mounted_here=yes
	fi
	mkdir -p "${mount_point}/dawos-campaign"
	load_file=${mount_point}/dawos-campaign/load.bin
	wanted=$((file_mb * 1048576))
	# The image has no stat: the size is the fifth field of "ls -ln".
	# shellcheck disable=SC2012 # one file, whose name we chose
	if [ "$(ls -ln "${load_file}" 2> /dev/null | awk '{ print $5 }')" != "${wanted}" ]; then
		echo "writing the load file (${file_mb} MiB)" > "${dir}/now.txt"
		rm -f "${load_file}"
		if ! taskset -c "${io_cpus}" fio --output-format=terse --name=layout --filename="${load_file}" \
			--size="${file_mb}m" --rw=write --bs=1M --ioengine=io_uring --direct=1 --iodepth=8 \
			--end_fsync=1 > "${RUN}/layout.txt" 2>&1; then
			echo "note: could not write ${load_file}: no disk load ($(tail -n 1 "${RUN}/layout.txt"))" >> "${dir}/notes.txt"
			disk=no
		fi
	fi
}

finish() {
	stop_children
	# shellcheck disable=SC2086 # process numbers, or nothing
	[ -n "${monitors:-}" ] && kill ${monitors} 2> /dev/null
	if [ -n "${tool_pid:-}" ]; then
		kill -INT "${tool_pid}" 2> /dev/null
		wait "${tool_pid}" 2> /dev/null
	fi
	remove_instances
	if [ "${mounted_here:-no}" = yes ]; then
		umount "${RUN}/shared" 2> /dev/null || umount -l "${RUN}/shared" 2> /dev/null
		rmdir "${RUN}/shared" 2> /dev/null
	fi
	rm -f "${RUN}"/*.txt "${RUN}"/*.err "${RUN}/rest.pid" "${RUN}/stop" "${dir}/now.txt"
}

# What the unit runs: the campaign of the directory $1.
run_campaign() {
	dir=$1
	plan=$(field "${dir}/meta.txt" plan)
	cycle_asked=$(field "${dir}/meta.txt" cycle)
	write_mbps=$(field "${dir}/meta.txt" write_mib_s)
	file_mb=$(field "${dir}/meta.txt" file_mib)
	disk=$(field "${dir}/meta.txt" disk_load)
	shared_dev=$(field "${dir}/meta.txt" shared_dev)
	vm_bytes=$(field "${dir}/meta.txt" vm_bytes)
	threshold_us=$(field "${dir}/meta.txt" threshold_us)
	trace_us=$(field "${dir}/meta.txt" trace_us)
	live_cpus=$(field "${dir}/meta.txt" live_cpus)
	system_cpus=$(field "${dir}/meta.txt" system_cpus)
	io_cpus=$(field "${dir}/meta.txt" io_cpus)
	other_cpus=$(field "${dir}/meta.txt" load_cpus)
	burst_cpus=$(field "${dir}/meta.txt" burst_cpus)
	quiet_cpus=$(field "${dir}/meta.txt" monitored_cpus)
	interface=$(network_interface)
	cpu_workers=$(($(expand_list "${other_cpus}" | wc -l) - 4))
	[ "${cpu_workers}" -ge 1 ] || cpu_workers=1
	burst_workers=$(expand_list "${burst_cpus}" | wc -l)
	power_workers=$(expand_list "${quiet_cpus}" | wc -l)
	load_file=
	mounted_here=no
	monitors=
	tool_pid=

	mkdir -p "${RUN}"
	rm -f "${RUN}/stop"
	trap 'finish' EXIT
	trap 'exit 1' INT TERM HUP
	prepare_disk

	number=0
	for item in ${plan}; do
		number=$((number + 1))
		kind=${item%%:*}
		duration=${item#*:}
		cycle=${cycle_asked}
		if [ "${cycle}" = default ]; then
			cycle=${CYCLE_SHORT}
			[ "${duration}" -ge "${CYCLE_LONG_FROM_S}" ] && cycle=${CYCLE_LONG}
		fi
		echo "step ${number} of $(echo "${plan}" | wc -w): ${kind} for ${duration} s, since $(date -u +%Y-%m-%dT%H:%M:%SZ)" > "${dir}/now.txt"
		echo "${number} ${kind} ${duration} $(date -u +%Y-%m-%dT%H:%M:%SZ) cycle: ${cycle}" >> "${dir}/steps.txt"
		run_step "${dir}/${number}-${kind}-${duration}" "${kind}" "${duration}"
		if [ -e "${RUN}/stop" ]; then
			echo "stopped: by hand during step ${number}, whose figures cover only a part of it" >> "${dir}/steps.txt"
			break
		fi
		if ! grep -q '^verdict: PASS' "${dir}/${number}-${kind}-${duration}/result.txt"; then
			echo "stopped: step ${number} did not pass, the steps after it were not run" >> "${dir}/steps.txt"
			break
		fi
		[ "${number}" -lt "$(echo "${plan}" | wc -w)" ] && sleep "${PAUSE_S}"
	done
	date -u '+finished_utc: %Y-%m-%dT%H:%M:%SZ' >> "${dir}/meta.txt"
	summarize "${dir}"
}

# Write summary.txt of the campaign in $1.
summarize() {
	result=$1
	{
		cat "${result}/meta.txt"
		echo
		echo "steps (delays in microseconds, of the threads in user space on the live CPUs):"
		printf '%-22s %11s %7s %9s %7s %7s %5s %4s %8s %7s %5s %6s %6s %4s  %s\n' \
			step samples avg p99.999 max irq_max traces smi throttle dev_irq link watt watt_max 'C' verdict
		for step in "${result}"/[0-9]*-*/; do
			[ -r "${step}result.txt" ] || continue
			name=${step%/}
			printf '%-22s %11s %7s %9s %7s %7s %5s %4s %8s %7s %5s %6s %6s %4s  %s\n' \
				"${name##*/}" \
				"$(field "${step}result.txt" samples)" \
				"$(field "${step}result.txt" usr_avg_us)" \
				"$(field "${step}result.txt" usr_p99999_us)" \
				"$(field "${step}result.txt" usr_max_us)" \
				"$(field "${step}result.txt" irq_max_us)" \
				"$(field "${step}result.txt" traces)" \
				"$(field "${step}result.txt" smi)" \
				"$(field "${step}result.txt" throttle)" \
				"$(field "${step}result.txt" device_interrupts_live)" \
				"$(field "${step}result.txt" link_changes)" \
				"$(field "${step}result.txt" package_watt_avg)" \
				"$(field "${step}result.txt" package_watt_max)" \
				"$(field "${step}result.txt" package_temperature_max_c)" \
				"$(field "${step}result.txt" verdict)"
		done
		for step in "${result}"/[0-9]*-*/; do
			[ -r "${step}phases.txt" ] || continue
			[ -r "${step}timerlat.txt" ] || continue
			name=${step%/}
			echo
			echo "${name##*/}: largest delay of each phase, by kind of phase (phases, lowest, median, highest, in us; package power and highest temperature in them):"
			awk '{ print $2 }' "${step}phases.txt" | sort -u | while read -r phase; do
				figures=$(awk -v phase="${phase}" '$2 == phase && $5 != "-" { print $5 }' "${step}phases.txt" | sort -n |
					awk '{ value[NR] = $1 } END { if (NR) printf "%d %d %d %d", NR, value[1], value[int((NR + 1) / 2)], value[NR]; else printf "0 - - -" }')
				power=$(awk -v phase="${phase}" -v readings="${step}turbostat.txt" '
					$2 == phase { first[++phases] = $3; last[phases] = $4 }
					END {
						while ((getline line < readings) > 0) {
							count = split(line, part, " ")
							if (part[1] == "Time_Of_Day_Seconds") { for (i = 1; i <= count; i++) column[part[i]] = i; continue }
							if (!("PkgWatt" in column)) continue
							for (i = 1; i <= phases; i++) if (part[1] + 0 >= first[i] + 5 && part[1] + 0 <= last[i]) {
								watt += part[column["PkgWatt"]]; n++
								if (part[column["PkgTmp"]] + 0 > hot) hot = part[column["PkgTmp"]] + 0
								break
							}
						}
						if (n) printf "%.1f W, %d C", watt / n, hot; else printf "-"
					}' "${step}phases.txt")
				# shellcheck disable=SC2086 # four figures
				printf '  %-8s %5s %7s %7s %7s   %s\n' "${phase}" ${figures} "${power}"
			done
			if [ -s "${step}interrupts-live.txt" ]; then
				echo "  interrupts on the live CPUs in the step (name, count): $(awk '{ printf "%s=%s ", $1, $2 }' "${step}interrupts-live.txt")"
			fi
			if [ -s "${step}fio.txt" ]; then
				echo "  fio, first and last phase: $(head -n 1 "${step}fio.txt"); $(tail -n 1 "${step}fio.txt")"
			fi
			if [ -s "${step}kernel-log.txt" ]; then
				echo "  kernel log: $(wc -l < "${step}kernel-log.txt") lines kept in kernel-log.txt"
			fi
		done
		echo
		grep -h '^stopped: ' "${result}/steps.txt" 2> /dev/null
		cat "${result}"/notes.txt "${result}"/[0-9]*-*/notes.txt 2> /dev/null
		if grep -q '^stopped: by hand' "${result}/steps.txt" 2> /dev/null; then
			echo "campaign: stopped by hand"
		elif grep -q '^finished_utc' "${result}/meta.txt"; then
			if grep -q '^stopped: ' "${result}/steps.txt" 2> /dev/null ||
				! grep -q '^verdict: PASS' "${result}"/[0-9]*-*/result.txt 2> /dev/null; then
				echo "campaign: FAIL"
			else
				echo "campaign: PASS ($(grep -l '^verdict: PASS' "${result}"/[0-9]*-*/result.txt | wc -l) of $(field "${result}/meta.txt" plan | wc -w) steps)"
			fi
		else
			echo "campaign: not finished"
		fi
	} > "${result}/summary.txt"
}

latest_dir() {
	latest=
	for candidate in "${BASE}"/campaign-*; do
		[ -d "${candidate}" ] && latest=${candidate}
	done
	echo "${latest}"
}

SELF=$(readlink -f "$0" 2> /dev/null)

case "${1:-}" in
-h | --help)
	usage
	exit 0
	;;
--status)
	last=$(latest_dir)
	if systemctl is-active --quiet "${UNIT}"; then
		echo "A campaign is running: ${last}"
		[ -r "${last}/now.txt" ] && cat "${last}/now.txt"
		for step in "${last}"/[0-9]*-*/; do
			[ -r "${step}result.txt" ] && echo "${step%/}: $(field "${step}result.txt" verdict), largest delay $(field "${step}result.txt" usr_max_us) us"
			if [ -r "${step}phases.txt" ] && [ ! -r "${step}result.txt" ]; then
				echo "last phases of the step (cycle, phase, from, to, largest delay in us):"
				tail -n 5 "${step}phases.txt" | sed 's/^/  /'
				[ -r "${step}trace-count" ] && echo "traces saved so far: $(cat "${step}trace-count")"
			fi
		done
	elif [ -n "${last}" ]; then
		echo "No campaign is running. Last one: ${last}"
		if [ -r "${last}/summary.txt" ]; then cat "${last}/summary.txt"; else cat "${last}/meta.txt"; fi
	else
		echo "No campaign is running, and none was run."
	fi
	exit 0
	;;
--stop)
	if ! systemctl is-active --quiet "${UNIT}"; then
		echo "${PROG}: no campaign is running"
		exit 0
	fi
	# The measuring tool is asked to end, so that it writes what it
	# has and leaves no tracer behind; the campaign then closes the
	# step and stops by itself.
	mkdir -p "${RUN}"
	: > "${RUN}/stop"
	pkill -INT -x rtla 2> /dev/null
	pkill -INT -x cyclictest 2> /dev/null
	pkill -x stress-ng 2> /dev/null
	pkill -x fio 2> /dev/null
	[ -r "${RUN}/rest.pid" ] && kill "$(cat "${RUN}/rest.pid")" 2> /dev/null
	waited=0
	while systemctl is-active --quiet "${UNIT}" && [ "${waited}" -lt 90 ]; do
		sleep 1
		waited=$((waited + 1))
		pkill -x stress-ng 2> /dev/null
		pkill -x fio 2> /dev/null
	done
	if systemctl is-active --quiet "${UNIT}"; then
		systemctl stop "${UNIT}" 2> /dev/null
		remove_instances
		last=$(latest_dir)
		if [ -n "${last}" ]; then
			echo "stopped: by hand, and not cleanly" >> "${last}/steps.txt"
			summarize "${last}"
		fi
	fi
	echo "${PROG}: stopped"
	exit 0
	;;
--summary)
	[ -d "${2:-}" ] || die "--summary needs a result directory"
	mkdir -p "${RUN}"
	summarize "$2"
	cat "$2/summary.txt"
	exit 0
	;;
--cleanup)
	systemctl is-active --quiet "${UNIT}" && die "a campaign is running"
	shared_dev=${2:-/dev/disk/by-partlabel/shared}
	[ -b "${shared_dev}" ] || die "${shared_dev} is not there"
	device=$(readlink -f "${shared_dev}")
	mount_point=$(awk -v device="${device}" '$1 == device { print $2; exit }' /proc/mounts)
	mounted_here=no
	if [ -z "${mount_point}" ]; then
		mount_point=${RUN}/shared
		mkdir -p "${mount_point}"
		mount "${shared_dev}" "${mount_point}" || die "could not mount ${shared_dev}"
		mounted_here=yes
	fi
	rm -rf "${mount_point}/dawos-campaign"
	[ "${mounted_here}" = yes ] && umount "${mount_point}"
	echo "${PROG}: load file removed from ${shared_dev}"
	exit 0
	;;
--run)
	run_campaign "$2"
	exit 0
	;;
--hit)
	record_hit "$2"
	exit 0
	;;
--start)
	shift
	;;
*)
	usage >&2
	exit 1
	;;
esac

# --- --start ------------------------------------------------------------------

[ $# -ge 1 ] || die "--start needs a plan: short, long, or a list of steps"
plan=$1
shift
preset=${plan}
case "${plan}" in
short) plan=${PLAN_SHORT} ;;
long) plan=${PLAN_LONG} ;;
trial) plan=${PLAN_TRIAL} ;;
esac
for item in ${plan}; do
	echo "${item}" | grep -Eq '^(timerlat|cyclictest):[0-9]+$' || die "bad step '${item}': timerlat:SECONDS or cyclictest:SECONDS"
	[ "${item#*:}" -ge 30 ] || die "a step lasts at least 30 seconds"
done

label=
write_mbps=25
file_mb=65536
disk=yes
shared_dev=/dev/disk/by-partlabel/shared
cycle=default
vm_bytes=2G
threshold_us=${THRESHOLD_US}
trace_us=${TRACE_US}
any_uptime=no
while [ $# -gt 0 ]; do
	case "$1" in
	--label)
		label=${2:-}
		shift 2
		;;
	--write-mbps)
		write_mbps=${2:-}
		shift 2
		;;
	--file-mb)
		file_mb=${2:-}
		shift 2
		;;
	--no-disk)
		disk=no
		shift
		;;
	--shared-dev)
		shared_dev=${2:-}
		shift 2
		;;
	--cycle)
		cycle=${2:-}
		shift 2
		;;
	--vm-bytes)
		vm_bytes=${2:-}
		shift 2
		;;
	--threshold-us)
		threshold_us=${2:-}
		shift 2
		;;
	--trace-us)
		trace_us=${2:-}
		shift 2
		;;
	--any-uptime)
		any_uptime=yes
		shift
		;;
	*)
		die "unknown option '$1' (try --help)"
		;;
	esac
done

[ "$(id -u)" = 0 ] || die "must run as root"
if [ -z "${SELF}" ] || [ ! -f "${SELF}" ]; then
	die "run it from a file on the machine, not from standard input"
fi
if [ -n "${label}" ]; then
	echo "${label}" | grep -Eq '^[A-Za-z0-9_-]+$' || die "the label may only contain letters, digits, '-' and '_'"
fi
echo "${write_mbps}" | grep -Eq '^[0-9]+$' || die "bad --write-mbps '${write_mbps}'"
echo "${file_mb}" | grep -Eq '^[0-9]+$' || die "bad --file-mb '${file_mb}'"
echo "${threshold_us}" | grep -Eq '^[0-9]+$' || die "bad --threshold-us '${threshold_us}'"
echo "${trace_us}" | grep -Eq '^[0-9]+$' || die "bad --trace-us '${trace_us}'"
[ "${trace_us}" -le "${threshold_us}" ] || trace_us=${threshold_us}
echo "${vm_bytes}" | grep -Eq '^[0-9]+[MG]$' || die "bad --vm-bytes '${vm_bytes}' (examples: 512M, 2G)"
[ "${preset}" = trial ] && [ "${cycle}" = default ] && cycle=${CYCLE_TRIAL}
if [ "${cycle}" != default ]; then
	for item in ${cycle}; do
		echo "${item}" | grep -Eq '^(full|power|burst-a|burst-b|burst-c|rest):[0-9]+$' ||
			die "bad phase '${item}': full, power, burst-a, burst-b, burst-c or rest, then :SECONDS"
	done
fi
for tool in rtla cyclictest stress-ng turbostat taskset systemd-run journalctl; do
	command -v "${tool}" > /dev/null || die "${tool} is not in this image"
done
for unit in "${UNIT}" dawos-hwnoise dawos-osnoise dawos-wakeup dawos-stream-test dawos-dspbench \
	dawos-thermal-sweep dawos-freq-sweep dawos-gpu-test dawos-cgroup-test dawos-starve-test; do
	systemctl is-active --quiet "${unit}" && die "${unit} is running: one measurement at a time"
done

[ -r "${TOPOLOGY}" ] || die "no ${TOPOLOGY}: the machine was not started with a boot profile"
live_cpus=$(domain live)
system_cpus=$(domain system)
io_cpus=$(domain io)
preemptive_cpus=$(domain preemptive)
if [ -z "${live_cpus}" ] || [ -z "${system_cpus}" ] || [ -z "${io_cpus}" ] || [ -z "${preemptive_cpus}" ]; then
	die "${TOPOLOGY} does not name the four domains"
fi
tune_status=$(sed -n 's/^  "status": "\([a-z]*\)".*/\1/p' "${TOPOLOGY}")
[ "${tune_status}" = ok ] || die "dawos-tune reports '${tune_status}', not 'ok': see ${TOPOLOGY}"
other_cpus="${preemptive_cpus},${system_cpus}"
quiet_cpus="${other_cpus},${io_cpus}"

# The P-cores that are not live: where the short bursts go.
performance_cpus=$(domain performance_cores)
expand_list "${preemptive_cpus}" > "/tmp/${PROG}.$$.a"
expand_list "${performance_cpus}" > "/tmp/${PROG}.$$.b"
# An empty list of patterns would match every line.
burst_cpus=
if [ -s "/tmp/${PROG}.$$.b" ]; then
	burst_cpus=$(grep -x -F -f "/tmp/${PROG}.$$.b" "/tmp/${PROG}.$$.a" | join_list)
fi
burst_note=
if [ -z "${burst_cpus}" ]; then
	burst_cpus=$(head -n 4 "/tmp/${PROG}.$$.a" | join_list)
	burst_note="the kind of the cores is not known here: bursts on the first CPUs of the preemptive domain"
fi
rm -f "/tmp/${PROG}.$$.a" "/tmp/${PROG}.$$.b"

if [ "${disk}" = yes ]; then
	command -v fio > /dev/null || die "fio is not in this image (or use --no-disk)"
	[ -b "${shared_dev}" ] || die "${shared_dev} is not there (or use --no-disk)"
	kind=$(blkid -o value -s TYPE "${shared_dev}" 2> /dev/null)
	[ "${kind}" = xfs ] || die "${shared_dev} holds '${kind:-nothing}', not XFS (or use --no-disk)"
	size_mb=$(($(blockdev --getsize64 "${shared_dev}") / 1048576))
	[ "${size_mb}" -gt $((file_mb + file_mb / 8)) ] || die "${shared_dev} is ${size_mb} MiB: too small for a file of ${file_mb} MiB"
fi

if [ "${any_uptime}" = no ] && [ "$(uptime_s)" -lt "${MIN_UPTIME_S}" ]; then
	die "the machine has been up for $(uptime_s) s: reference measurements start after ${MIN_UPTIME_S} s"
fi

total=0
for item in ${plan}; do
	total=$((total + ${item#*:} + PAUSE_S + 15))
done

mkdir -p "${BASE}" || die "could not create ${BASE}"
dir="${BASE}/campaign-$(date -u +%Y%m%dT%H%M%SZ)${label:+-${label}}"
mkdir -p "${dir}" || die "could not create ${dir}"
{
	date -u '+started_utc: %Y-%m-%dT%H:%M:%SZ'
	echo "label: ${label:-none}"
	echo "plan: ${plan}"
	echo "cycle: ${cycle}"
	echo "script_sha256: $(sha256sum < "${SELF}" | cut -d ' ' -f 1)"
	echo "threshold_us: ${threshold_us}"
	echo "trace_us: ${trace_us}"
	echo "timerlat_period_us: ${PERIOD_US}"
	echo "cyclictest_period_us: ${CYCLICTEST_PERIOD_US}"
	echo "priority: fifo ${PRIORITY}"
	echo "live_cpus: ${live_cpus}"
	echo "system_cpus: ${system_cpus}"
	echo "io_cpus: ${io_cpus}"
	echo "load_cpus: ${other_cpus}"
	echo "burst_cpus: ${burst_cpus}"
	echo "monitored_cpus: ${quiet_cpus}"
	echo "disk_load: ${disk}"
	echo "shared_dev: ${shared_dev}"
	echo "write_mib_s: ${write_mbps}"
	echo "file_mib: ${file_mb}"
	echo "vm_bytes: ${vm_bytes}"
	echo "uptime_at_start_s: $(uptime_s)"
	# The fingerprints of what is under test.
	echo "image_version: $(sed -n 's/^IMAGE_VERSION=//p' /etc/os-release)"
	echo "image_build: $(sed -n 's/^BUILD_ID=//p' /etc/os-release)"
	echo "kernel: $(uname -srv)"
	echo "kernel_config_sha256: $(zcat /proc/config.gz 2> /dev/null | sha256sum | cut -d ' ' -f 1)"
	echo "cmdline: $(cat /proc/cmdline)"
	echo "cmdline_sha256: $(sha256sum < /proc/cmdline | cut -d ' ' -f 1)"
	echo "bios: $(cat /sys/class/dmi/id/bios_version 2> /dev/null) $(cat /sys/class/dmi/id/bios_date 2> /dev/null)"
	echo "microcode: $(sed -n 's/^microcode[^:]*: //p' /proc/cpuinfo | head -n 1)"
	echo "profile: $(sed -n 's/.*dawos\.profile=\([a-z0-9-]*\).*/\1/p' /proc/cmdline)"
	echo "tune: ${tune_status}"
	echo "modules_loaded: $(($(wc -l < /proc/modules)))"
	echo "tools: rtla $(rtla timerlat hist --help 2>&1 | sed -n 's/.*(version \(.*\)).*/\1/p' | head -n 1); $(stress-ng --version 2> /dev/null); $(fio --version 2> /dev/null); $(cyclictest --help 2>&1 | head -n 1)"
	counts=$(throttle_counts)
	[ -n "${counts}" ] && echo "throttle_since_boot: ${counts}"
	[ -n "${burst_note}" ] && echo "note: ${burst_note}"
} > "${dir}/meta.txt"
cp "${TOPOLOGY}" "${dir}/topology.json"

# The campaign runs from a copy of its own: this file may be replaced
# by a newer one while it runs, and a shell reads its script as it goes.
mkdir -p "${RUN}"
copy="${RUN}/campaign-$(date -u +%Y%m%dT%H%M%SZ).sh"
cp "${SELF}" "${copy}" || die "could not copy ${SELF} to ${RUN}"
systemd-run --quiet --unit="${UNIT}" --collect \
	--description="dawOS measurement campaign (P0.7)" \
	/bin/sh "${copy}" --run "${dir}" ||
	die "could not start the campaign"

echo "Campaign started: ${plan}"
echo "It takes about $((total / 60)) minutes$([ "${disk}" = yes ] && echo ", plus the time to write the load file the first time")."
echo "Results in ${dir}"
echo "Follow it with: sh ${SELF} --status"
