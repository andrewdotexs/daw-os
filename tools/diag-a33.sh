#!/bin/sh
# Second round of the diagnosis of open point A-33: why a live CPU at
# rest wakes up late while the other performance cores work in bursts.
#
# This script runs ON the dawOS machine, as root, with the tools of its
# image (BusyBox sh). From the development PC:
#
#   ssh "root@HOST" sh -s < tools/diag-a33.sh
#
# What the first round found (step P0.6d, part 2): with the encryption
# work of dm-crypt on the performance cores of the preemptive domain,
# the timer interrupt of a live CPU at rest arrives up to 55 us late;
# on the efficiency cores it does not, and a live CPU that is busy is
# not touched at all. The suspicion: those cores go from rest to their
# top frequency and back hundreds of times a second, each change needs
# the processor to move its voltage, and a core that asks to wake up in
# that moment has to wait.
#
# So the same load is repeated while the frequency of those cores is
# held in different ways, and then a load that has nothing to do with
# encryption is tried:
#
#   crypt-default      encryption on those cores, frequency as it is
#   crypt-max-live     their top frequency lowered to that of the live CPUs
#   crypt-fixed-top    their frequency fixed at the top
#   crypt-fixed-live   their frequency fixed at that of the live CPUs
#   crypt-performance  energy preference "performance"
#   burst-default      stress-ng in bursts of 1 ms, 12 % of the time
#   steady-default     stress-ng without pause
#
# The live CPUs are at rest in every case. Each case takes about two
# minutes; the settings are put back at the end, also if interrupted.
# A table is printed and kept in /var/lib/dawos/measure/a33-<time>.
#
# For a trial where the real machine is not at hand (a virtual machine):
#   A33_CPUS=1-3 A33_HOMES_DEV=/dev/loopN A33_QUICK=1

set -u

PROG=diag-a33
CPU_DIR=/sys/devices/system/cpu
BASE=/var/lib/dawos/measure

die() {
	echo "${PROG}: $*" >&2
	exit 1
}

# CPUs of a kernel list ("4-7", "0-3,8-15"), one per line, sorted.
expand_list() {
	echo "$1" | tr ',' '\n' | while IFS=- read -r first last; do
		case "${first}" in
		'' | *[!0-9]*) continue ;;
		esac
		seq "${first}" "${last:-${first}}"
	done | sort -n -u
}

compress_list() {
	sort -n -u | awk '
		NR == 1 { start = prev = $1; next }
		$1 == prev + 1 { prev = $1; next }
		{ printf "%s%s", sep, (start == prev ? start : start "-" prev); sep = ","; start = prev = $1 }
		END { if (NR) printf "%s%s", sep, (start == prev ? start : start "-" prev); print "" }'
}

[ "$(id -u)" = 0 ] || die "must run as root"
for tool in dawos-stream-test dawos-wakeup stress-ng; do
	command -v "${tool}" > /dev/null || die "'${tool}' not found: this needs the dev variant of the image"
done
dawos-stream-test --help | grep -q crypt-cpus || die "this image is too old: dawos-stream-test has no --crypt-cpus"
for other in dawos-stream-test dawos-wakeup dawos-osnoise dawos-hwnoise dawos-freq-sweep dawos-thermal-sweep; do
	systemctl is-active --quiet "${other}" && die "'${other}' is running: wait for it, or stop it"
done

# The performance cores of the preemptive domain, and the live CPUs.
live_list=$(grep -v null "${CPU_DIR}/isolated" 2> /dev/null)
live_cpu=$(expand_list "${live_list}" | head -n 1)
cpus=${A33_CPUS:-}
if [ -z "${cpus}" ]; then
	domain=$(sed -n 's/^    "preemptive": "\(.*\)".*/\1/p' /run/dawos/topology.json 2> /dev/null)
	performance=$(cat /sys/devices/cpu_core/cpus 2> /dev/null)
	cpus=$(for cpu in $(expand_list "${domain}"); do
		expand_list "${performance}" | grep -qx "${cpu}" && echo "${cpu}"
	done | compress_list)
fi
[ -n "${cpus}" ] || die "no performance core found in the preemptive domain (is the boot profile active?)"
[ -n "${live_cpu}" ] || die "no live CPU: this needs a boot profile"
live_khz=$(cat "${CPU_DIR}/cpu${live_cpu}/cpufreq/scaling_max_freq" 2> /dev/null)
count=$(expand_list "${cpus}" | wc -l)

if [ -n "${A33_QUICK:-}" ]; then
	crypt_seconds=25
	wakeup_seconds=20
	quick=--quick
else
	crypt_seconds=100
	wakeup_seconds=90
	quick=
fi

dir="${BASE}/a33-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "${dir}/saved" || die "could not create ${dir}"

# The frequency settings of the cores, as found, to put them back.
for cpu in $(expand_list "${cpus}"); do
	for file in scaling_min_freq scaling_max_freq energy_performance_preference; do
		cat "${CPU_DIR}/cpu${cpu}/cpufreq/${file}" > "${dir}/saved/${cpu}-${file}" 2> /dev/null
	done
done

# Give the cores a lowest and a highest frequency and an energy
# preference; "-" leaves a setting as it was found.
set_cores() {
	for cpu in $(expand_list "${cpus}"); do
		cpufreq="${CPU_DIR}/cpu${cpu}/cpufreq"
		[ -d "${cpufreq}" ] || continue
		low=$1
		high=$2
		preference=$3
		[ "${low}" != - ] || low=$(cat "${dir}/saved/${cpu}-scaling_min_freq")
		[ "${high}" != - ] || high=$(cat "${dir}/saved/${cpu}-scaling_max_freq")
		[ "${preference}" != - ] || preference=$(cat "${dir}/saved/${cpu}-energy_performance_preference")
		# The lowest goes down first, so that any highest is accepted.
		cat "${cpufreq}/cpuinfo_min_freq" > "${cpufreq}/scaling_min_freq" 2> /dev/null
		echo "${high}" > "${cpufreq}/scaling_max_freq" 2> /dev/null
		echo "${low}" > "${cpufreq}/scaling_min_freq" 2> /dev/null
		[ -z "${preference}" ] || echo "${preference}" > "${cpufreq}/energy_performance_preference" 2> /dev/null
	done
}

# "lowest-highest kHz, preference" as the first core has them now.
cores_now() {
	cpufreq="${CPU_DIR}/cpu$(expand_list "${cpus}" | head -n 1)/cpufreq"
	if [ -d "${cpufreq}" ]; then
		echo "$(($(cat "${cpufreq}/scaling_min_freq") / 1000))-$(($(cat "${cpufreq}/scaling_max_freq") / 1000))MHz,$(cat "${cpufreq}/energy_performance_preference" 2> /dev/null)"
	else
		echo "no-cpufreq"
	fi
}

load_pid=
cleanup() {
	[ -z "${load_pid}" ] || kill "${load_pid}" 2> /dev/null
	set_cores - - -
	rm -rf "${dir}/saved"
}
trap cleanup EXIT
trap 'exit 1' INT TERM HUP

# While the unit $1 runs: every 5 seconds, the highest frequency seen
# on the cores (those that have a tick; the others give a stale value).
# Prints that frequency in MHz when the unit has ended.
watch_unit() {
	seen=0
	while systemctl is-active --quiet "$1"; do
		for cpu in $(expand_list "${cpus}"); do
			now=$(cat "${CPU_DIR}/cpu${cpu}/cpufreq/scaling_cur_freq" 2> /dev/null || echo 0)
			[ "${now}" -le "${seen}" ] || seen=${now}
		done
		sleep 5
	done
	echo $((seen / 1000))
}

# The last result directory whose name matches the pattern $1.
latest() {
	found=
	for candidate in "${BASE}"/$1; do
		[ -d "${candidate}" ] && found=${candidate}
	done
	echo "${found}"
}

printf '%-18s %-36s %-11s %-11s %-9s %s\n' case cores_setting irq_max_us usr_max_us mhz_seen busy_pct > "${dir}/table.txt"

# One line of the table: $1 case, $2 irq_max, $3 usr_max, $4 MHz seen,
# $5 how busy the cores were.
row() {
	printf '%-18s %-36s %-11s %-11s %-9s %s\n' "$1" "$(cores_now)" "$2" "$3" "$4" "$5" | tee -a "${dir}/table.txt"
}

# Encryption on the cores: the streaming load of dawos-stream-test on
# the loop configuration, which erases nothing.
crypt_case() {
	name=$1
	# shellcheck disable=SC2086
	if ! dawos-stream-test ${quick} --configs l --loads w2 --w2-seconds "${crypt_seconds}" --crypt-cpus "${cpus}" \
		${A33_HOMES_DEV:+--homes-dev "${A33_HOMES_DEV}"} "a33-${name}" > "${dir}/${name}.log" 2>&1; then
		row "${name}" failed "$(head -n 1 "${dir}/${name}.log" | cut -c 1-60)" - -
		return
	fi
	sleep 5
	mhz=$(watch_unit dawos-stream-test)
	result=$(latest "stream-*-a33-${name}")
	irq=$(awk '{ print $1 }' "${result}/timerlat-l.max" 2> /dev/null)
	usr=$(awk '{ print $3 }' "${result}/timerlat-l.max" 2> /dev/null)
	busy=$(for cpu in $(expand_list "${cpus}"); do
		awk -v wanted="cpu${cpu}" '$1 == wanted { print $2 }' "${result}/cpus-l-w2.txt" 2> /dev/null
	done | awk '{ sum += $1; n++ } END { if (n) printf "%.1f", sum / n; else print "-" }')
	row "${name}" "${irq:--}" "${usr:--}" "${mhz}" "${busy}"
}

# A load that is not encryption: stress-ng on the cores, busy $2 percent
# of the time in slices of 1 ms, while dawos-wakeup measures the live
# CPUs at rest.
stress_case() {
	name=$1
	stress-ng --taskset "${cpus}" --temp-path "${dir}" --cpu "${count}" --cpu-load "$2" --cpu-load-slice 1 \
		--timeout 0 > "${dir}/${name}.log" 2>&1 &
	load_pid=$!
	sleep 3
	if dawos-wakeup --cases idle-quiet --step "${wakeup_seconds}" "a33-${name}" >> "${dir}/${name}.log" 2>&1; then
		sleep 5
		mhz=$(watch_unit dawos-wakeup)
		result=$(latest "wakeup-*-a33-${name}")
		irq=$(awk '$1 == "idle-quiet" { print $6 }' "${result}/summary.txt" 2> /dev/null)
		usr=$(awk '$1 == "idle-quiet" { print $11 }' "${result}/summary.txt" 2> /dev/null)
		row "${name}" "${irq:--}" "${usr:--}" "${mhz}" "$2"
	else
		row "${name}" failed "$(tail -n 1 "${dir}/${name}.log" | cut -c 1-60)" - -
	fi
	kill "${load_pid}" 2> /dev/null
	wait "${load_pid}" 2> /dev/null
	load_pid=
	sleep 3
}

{
	date -u '+started_utc: %Y-%m-%dT%H:%M:%SZ'
	echo "cores: ${cpus}"
	echo "live_cpus: ${live_list}"
	echo "live_khz: ${live_khz:-unknown}"
	echo "seconds_per_case: encryption ${crypt_seconds}, stress ${wakeup_seconds}"
	echo "kernel: $(uname -srv)"
	echo "image: $(sed -n 's/^BUILD_ID=//p' /etc/os-release)"
	echo "modules_loaded: $(lsmod | awk 'NR > 1 { print $1 }' | tr '\n' ' ')"
} > "${dir}/meta.txt"

echo "${PROG}: cores ${cpus}, live CPUs ${live_list} at ${live_khz:-?} kHz; seven cases, about two minutes each"
head -n 1 "${dir}/table.txt"

top=$(cat "${CPU_DIR}/cpu$(expand_list "${cpus}" | head -n 1)/cpufreq/cpuinfo_max_freq" 2> /dev/null || echo -)

crypt_case crypt-default
set_cores - "${live_khz:--}" -
crypt_case crypt-max-live
set_cores "${top}" "${top}" -
crypt_case crypt-fixed-top
set_cores "${live_khz:--}" "${live_khz:--}" -
crypt_case crypt-fixed-live
set_cores - - performance
crypt_case crypt-performance
set_cores - - -
stress_case burst-default 12
stress_case steady-default 100

date -u '+finished_utc: %Y-%m-%dT%H:%M:%SZ' >> "${dir}/meta.txt"
echo "${PROG}: done; the table is in ${dir}/table.txt"
