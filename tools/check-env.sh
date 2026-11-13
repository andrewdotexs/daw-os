#!/usr/bin/env bash
# Check that this machine can build dawOS with Buildroot and run it in
# QEMU. This is the acceptance test of step P0.1.
#
# Prints one line per check (OK / WARN / FAIL) and exits non-zero if any
# check failed. Warnings do not fail the run.

set -uo pipefail

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# Tag the Buildroot submodule is expected to sit on (ADR 0007).
readonly BUILDROOT_TAG=2026.02.3
# Space needed by a first full build: output tree plus download cache.
readonly MIN_FREE_GIB=40
# Below this the build is at risk even with a single job (kernel link).
readonly MIN_RAM_GIB=4

failures=0
warnings=0

ok() { printf '  OK    %s\n' "$*"; }
warn() {
	printf '  WARN  %s\n' "$*"
	warnings=$((warnings + 1))
}
fail() {
	printf '  FAIL  %s\n' "$*"
	failures=$((failures + 1))
}

echo "dawOS build environment check"

# --- Where we are running ---------------------------------------------------
in_wsl=no
if grep -qi microsoft /proc/version 2> /dev/null; then
	in_wsl=yes
	ok "running inside WSL2"
	# The Windows drives are slow and case-insensitive: builds break there.
	case ${repo_dir} in
	/mnt/[a-z] | /mnt/[a-z]/*) fail "repository is on a Windows drive (${repo_dir}): clone it under \$HOME" ;;
	*) ok "repository is on the Linux filesystem (${repo_dir})" ;;
	esac
else
	ok "running on native Linux"
fi

# --- PATH -------------------------------------------------------------------
# Buildroot refuses a PATH containing spaces, tabs or newlines; the Windows
# PATH that WSL appends by default has plenty.
case ${PATH} in
*[[:space:]]*) fail "PATH contains whitespace: set appendWindowsPath=false in /etc/wsl.conf" ;;
*) ok "PATH has no whitespace" ;;
esac

case ${repo_dir} in
*[[:space:]]*) fail "repository path contains whitespace (${repo_dir})" ;;
*) ok "repository path has no whitespace" ;;
esac

# --- Tools ------------------------------------------------------------------
# Buildroot's mandatory host packages, plus what our own targets call.
required_tools=(
	make gcc g++ patch perl python3 rsync bc cpio unzip file wget git
	tar gzip bzip2 xz sed awk find diff which
	qemu-system-x86_64 shellcheck
	# openssh-client: host key of dev images, login to the real machine.
	ssh ssh-keygen
)
missing=()
for tool in "${required_tools[@]}"; do
	command -v "${tool}" > /dev/null 2>&1 || missing+=("${tool}")
done
if ((${#missing[@]} == 0)); then
	ok "all required tools found"
else
	fail "missing tools: ${missing[*]}"
fi

if command -v npx > /dev/null 2>&1; then
	ok "npx found (markdownlint for 'make lint')"
else
	warn "npx not found: 'make lint' cannot run markdownlint (package 'npm')"
fi

# Headers needed to build kernel host tools and the menuconfig interface.
for header in ncurses.h openssl/ssl.h libelf.h; do
	if [[ -f /usr/include/${header} ]] || compgen -G "/usr/include/*/${header}" > /dev/null; then
		ok "header ${header} found"
	else
		fail "header ${header} missing (libncurses-dev, libssl-dev, libelf-dev)"
	fi
done

# --- Virtualisation ---------------------------------------------------------
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
	ok "/dev/kvm is usable"
elif [[ -e /dev/kvm ]]; then
	fail "/dev/kvm exists but is not accessible: add your user to group 'kvm' and log in again"
else
	fail "/dev/kvm missing: set nestedVirtualization=true in .wslconfig, then 'wsl --shutdown'"
fi

if compgen -G "/usr/share/OVMF/OVMF_CODE*.fd" > /dev/null ||
	compgen -G "/usr/share/edk2/*/OVMF_CODE*.fd" > /dev/null; then
	ok "OVMF firmware found"
else
	fail "OVMF firmware missing (package 'ovmf')"
fi

# --- Buildroot submodule ----------------------------------------------------
if [[ -f ${repo_dir}/buildroot/Makefile ]]; then
	tag=$(git -C "${repo_dir}/buildroot" describe --tags --exact-match 2> /dev/null || echo none)
	if [[ ${tag} == "${BUILDROOT_TAG}" ]]; then
		ok "Buildroot submodule at ${tag}"
	else
		fail "Buildroot submodule is at '${tag}', expected ${BUILDROOT_TAG}: run 'git submodule update --init'"
	fi
else
	fail "Buildroot submodule not fetched: run 'git submodule update --init'"
fi

# --- Resources --------------------------------------------------------------
free_gib=$(df --output=avail -BG "${repo_dir}" | tail -n 1 | tr -dc '0-9')
if ((free_gib >= MIN_FREE_GIB)); then
	ok "${free_gib} GiB free on the repository filesystem"
else
	fail "${free_gib} GiB free, at least ${MIN_FREE_GIB} GiB needed"
fi

# Inside WSL2 the figure above is the ceiling of the virtual disk, not real
# space: the virtual disk grows on a Windows drive, normally C:. Check that
# one too, when it is mounted.
if [[ ${in_wsl} == yes ]] && mountpoint -q /mnt/c 2> /dev/null; then
	host_free_gib=$(df --output=avail -BG /mnt/c 2> /dev/null | tail -n 1 | tr -dc '0-9')
	if [[ -z ${host_free_gib} ]]; then
		warn "cannot read the free space of Windows drive C:"
	elif ((host_free_gib >= MIN_FREE_GIB)); then
		ok "${host_free_gib} GiB free on Windows drive C: (where the WSL virtual disk normally grows)"
	else
		warn "${host_free_gib} GiB free on Windows drive C:, ${MIN_FREE_GIB} GiB needed if the WSL virtual disk lives there"
	fi
fi

# Buildroot compiles with one job more than the CPUs it sees. As a rule of
# thumb each job needs about 1 GiB at the worst moments (an estimate, not
# a measurement): with less than that, the kernel or systemd build can be
# killed for lack of memory. Suggest a job count that fits.
ram_gib=$(awk '/^MemTotal:/ { printf "%d", $2 / 1024 / 1024 }' /proc/meminfo)
cpus=$(nproc)
default_jobs=$((cpus + 1))
if ((ram_gib < MIN_RAM_GIB)); then
	fail "${ram_gib} GiB of RAM visible, at least ${MIN_RAM_GIB} GiB needed (raise 'memory' in .wslconfig)"
elif ((ram_gib < default_jobs)); then
	warn "${ram_gib} GiB of RAM for ${cpus} CPUs: build with 'make image JOBS=$((ram_gib - 1))' or raise 'memory' in .wslconfig"
else
	ok "${ram_gib} GiB of RAM visible, ${cpus} CPUs"
fi

# --- Verdict ----------------------------------------------------------------
echo
if ((failures > 0)); then
	echo "RESULT: FAIL (${failures} failed, ${warnings} warnings)"
	exit 1
fi
echo "RESULT: PASS (${warnings} warnings)"
