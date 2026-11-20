#!/usr/bin/env bash
# dawOS post-build hook: runs after all packages are installed in the
# target tree and before the filesystem images are created.
#
# Called by Buildroot as:  post-build.sh <TARGET_DIR> <variant>
#
# It stamps the image identity and adds the few things that come from this
# machine rather than from the repository: SSH keys and the static network
# configuration. Everything else belongs to a package or to the overlay.
#
# Nothing secret or machine-specific is read from the repository itself:
#   keys/   SSH host key of dev images and the key that signs updates,
#           generated here on first use
#   local/  authorized SSH keys, network settings of the reference machine
# Both directories are ignored by git.

set -euo pipefail

target_dir=${1:?usage: post-build.sh <TARGET_DIR> <variant>}
variant=${2:-dev}

ext_dir=${BR2_EXTERNAL_DAWOS_PATH:?BR2_EXTERNAL_DAWOS_PATH is not set}
repo_dir=$(cd "${ext_dir}/.." && pwd)

version=$(tr -d '[:space:]' < "${repo_dir}/VERSION")
[[ ${version} =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
	echo "post-build: VERSION must look like 1.2.3, got '${version}'" >&2
	exit 1
}

# Version of this image. systemd-sysupdate installs an image only if its
# version is newer than what the machine has, and the version also names
# the partitions and the unified kernel image. Every dev build therefore
# gets its own: the release number followed by the UTC time of the build,
# which keeps growing. A release (prod) image carries the bare number.
# DAWOS_VERSION_STAMP overrides the time, to reproduce a build.
image_version=${version}
if [[ ${variant} == dev ]]; then
	stamp=${DAWOS_VERSION_STAMP:-$(date -u +%Y%m%d%H%M%S)}
	[[ ${stamp} =~ ^[0-9]{14}$ ]] || {
		echo "post-build: DAWOS_VERSION_STAMP must be 14 digits (YYYYMMDDhhmmss), got '${stamp}'" >&2
		exit 1
	}
	image_version=${version}.${stamp}
fi
# post-image.sh, which runs later in the same build, reads it from here.
install -d -m 0755 "${BINARIES_DIR:?BINARIES_DIR is not set}"
printf '%s\n' "${image_version}" > "${BINARIES_DIR}/version.txt"

# --- The image must contain what the system relies on --------------------------
# Each of these comes from a package option. Buildroot does not rebuild a
# package when only its options change, so an option added to the
# defconfig can be missing from the image without any error: better to
# stop here, naming the package, than to find out on the machine.
while read -r program package; do
	[[ -e ${target_dir}${program} ]] || {
		echo "post-build: ${program} is missing from the image." >&2
		echo "post-build: the package '${package}' was built before one of its options was enabled:" >&2
		echo "post-build: run 'make br-${package}-dirclean' and then 'make image' again." >&2
		exit 1
	}
done <<'REQUIRED'
/usr/lib/systemd/systemd-sysupdate systemd
/usr/lib/systemd/systemd-pull systemd
/usr/lib/systemd/systemd-bless-boot systemd
/usr/lib/systemd/boot/efi/linuxx64.efi.stub systemd
/usr/bin/systemd-repart systemd
/usr/bin/gpg gnupg2
/usr/sbin/veritysetup cryptsetup
/usr/sbin/dmsetup lvm2
REQUIRED

# Exact source revision, for bug reports and measurement reports. Falls
# back gracefully when the tree is not a git checkout (release tarball).
build_id=$(git -C "${repo_dir}" describe --always --dirty --abbrev=12 2>/dev/null || echo unknown)

# os-release(5). IMAGE_ID and IMAGE_VERSION are the fields systemd-sysupdate
# and the boot report read; VARIANT_ID tells "dev" from "prod".
install -d -m 0755 "${target_dir}/usr/lib"
cat > "${target_dir}/usr/lib/os-release" <<OSRELEASE
NAME="dawOS"
ID=dawos
PRETTY_NAME="dawOS ${image_version} (${variant})"
VERSION="${version}"
VERSION_ID=${version}
VARIANT_ID=${variant}
IMAGE_ID=dawos
IMAGE_VERSION=${image_version}
BUILD_ID=${build_id}
OSRELEASE
ln -sfn ../usr/lib/os-release "${target_dir}/etc/os-release"

# --- Key that signs updates ----------------------------------------------------
# systemd-sysupdate accepts a new image only if the list of checksums that
# comes with it is signed by a key of this keyring. The private half stays
# in keys/update/ and signs the release in post-image.sh; the public half
# goes into the image. Like the Secure Boot key, the dev one is created on
# the first build, unprotected, and then reused.
command -v gpg > /dev/null || {
	echo "post-build: gpg not found: sudo apt install gnupg" >&2
	exit 1
}
update_home=${repo_dir}/keys/update/gnupg
if [[ ! -d ${update_home} ]]; then
	[[ ${variant} == dev ]] || {
		echo "post-build: no update signing key in ${update_home}, and only the dev variant creates one" >&2
		exit 1
	}
	echo "post-build: creating the development update signing key in ${update_home}"
	install -d -m 0700 "${update_home}"
	gpg --homedir "${update_home}" --batch --quiet --passphrase '' \
		--quick-generate-key "dawOS development update key" ed25519 sign never
fi
# systemd 258 reads the vendor keyring from import-pubring.pgp (earlier
# versions: .gpg). The systemd package installs one with the keys of some
# distributions: ours replaces it, so that nothing else is trusted.
rm -f "${target_dir}/usr/lib/systemd/import-pubring.gpg"
install -D -m 0644 /dev/null "${target_dir}/usr/lib/systemd/import-pubring.pgp"
gpg --homedir "${update_home}" --batch --quiet --export > "${target_dir}/usr/lib/systemd/import-pubring.pgp"
[[ -s ${target_dir}/usr/lib/systemd/import-pubring.pgp ]] || {
	echo "post-build: could not export the update public key from ${update_home}" >&2
	exit 1
}
echo "post-build: update public key installed in /usr/lib/systemd/import-pubring.pgp"

# --- SSH access (dev variant only, ADR 0005) ----------------------------------
if [[ ${variant} == dev ]]; then
	keys_dir=${repo_dir}/keys
	local_dir=${repo_dir}/local

	# Host key. The root filesystem is read-only, so the key cannot be
	# generated on the machine at first boot: a dev image carries one made
	# on the build host and kept out of the repository. Every dev image
	# built from this checkout therefore identifies itself the same way.
	host_key=${keys_dir}/ssh_host_ed25519_key
	if [[ ! -f ${host_key} ]]; then
		command -v ssh-keygen > /dev/null || {
			echo "post-build: ssh-keygen not found (package openssh-client)" >&2
			exit 1
		}
		install -d -m 0700 "${keys_dir}"
		ssh-keygen -q -t ed25519 -N '' -C 'dawos-dev-host' -f "${host_key}"
		echo "post-build: generated a new SSH host key in ${keys_dir}"
	fi
	install -D -m 0600 "${host_key}" "${target_dir}/etc/ssh/ssh_host_ed25519_key"
	install -D -m 0644 "${host_key}.pub" "${target_dir}/etc/ssh/ssh_host_ed25519_key.pub"

	# Keys allowed to log in as root. Comment lines are dropped; without
	# any key the image is still usable from its console and in QEMU.
	authorized=${local_dir}/ssh-authorized-keys
	rm -rf "${target_dir}/root/.ssh"
	if [[ -f ${authorized} ]] && grep -qEv '^[[:space:]]*(#|$)' "${authorized}"; then
		install -d -m 0700 "${target_dir}/root/.ssh"
		grep -Ev '^[[:space:]]*(#|$)' "${authorized}" > "${target_dir}/root/.ssh/authorized_keys"
		chmod 0600 "${target_dir}/root/.ssh/authorized_keys"
		echo "post-build: SSH keys installed from ${authorized}"
	else
		echo "post-build: no key in ${authorized}: SSH login will not be possible"
	fi
fi

# --- Static network of the reference machine -----------------------------------
# Read from local/network-285k.conf (see local.example/). The settings are
# bound to one kernel driver, so that every other wired interface, the
# QEMU one included, keeps using DHCP (80-dawos-ether.network).
net_conf=${repo_dir}/local/network-285k.conf
net_unit=${target_dir}/usr/lib/systemd/network/70-dawos-static.network
rm -f "${net_unit}"
if [[ -f ${net_conf} ]]; then
	driver=''
	address=''
	gateway=''
	dns=''
	while IFS='=' read -r key value; do
		# Skip comments and blank lines; trim the value.
		[[ ${key} =~ ^[[:space:]]*(#|$) ]] && continue
		key=${key//[[:space:]]/}
		value=${value#"${value%%[![:space:]]*}"}
		value=${value%"${value##*[![:space:]]}"}
		case ${key} in
		DAWOS_NET_DRIVER) driver=${value} ;;
		DAWOS_NET_ADDRESS) address=${value} ;;
		DAWOS_NET_GATEWAY) gateway=${value} ;;
		DAWOS_NET_DNS) dns=${value} ;;
		*)
			echo "post-build: unknown key '${key}' in ${net_conf}" >&2
			exit 1
			;;
		esac
	done < "${net_conf}"

	# Validate: these values end up in a configuration file of the image.
	ipv4='([0-9]{1,3}\.){3}[0-9]{1,3}'
	[[ ${driver} =~ ^[a-z0-9_]+$ ]] || { echo "post-build: bad DAWOS_NET_DRIVER '${driver}'" >&2; exit 1; }
	[[ ${address} =~ ^${ipv4}/[0-9]{1,2}$ ]] || { echo "post-build: bad DAWOS_NET_ADDRESS '${address}' (expected a.b.c.d/nn)" >&2; exit 1; }
	[[ ${gateway} =~ ^${ipv4}$ ]] || { echo "post-build: bad DAWOS_NET_GATEWAY '${gateway}'" >&2; exit 1; }
	[[ ${dns} =~ ^${ipv4}([[:space:]]+${ipv4})*$ ]] || { echo "post-build: bad DAWOS_NET_DNS '${dns}'" >&2; exit 1; }

	# The example file carries documentation addresses (RFC 5737). A copy
	# left unedited would give the machine an address nobody can reach, and
	# the mistake would only show up on the real hardware: stop here.
	for value in "${address}" "${gateway}" ${dns}; do
		case ${value} in
		192.0.2.* | 198.51.100.* | 203.0.113.*)
			echo "post-build: ${net_conf} still contains the example address ${value}." >&2
			echo "post-build: edit it and put in the real address, gateway and DNS." >&2
			exit 1
			;;
		esac
	done

	cat > "${net_unit}" <<NETWORK
# Generated by post-build.sh from local/network-285k.conf.
# Sorts before 80-dawos-ether.network, so it wins for this driver.
[Match]
Driver=${driver}

[Network]
Address=${address}
Gateway=${gateway}
DNS=${dns}
NETWORK
	echo "post-build: static network configuration for driver '${driver}' installed: ${address}, gateway ${gateway}, DNS ${dns}"
else
	echo "post-build: no ${net_conf}: DHCP on every wired interface"
fi

# --- Firmware of the GPU --------------------------------------------------------
# The package installs the files of every Intel GPU generation (tens of
# megabytes). The driver asks for those of the processor family only:
# mtl_* covers Meteor Lake and Arrow Lake.
gpu_firmware=${target_dir}/lib/firmware/i915
if [[ -d ${gpu_firmware} ]]; then
	find "${gpu_firmware}" \( -type f -o -type l \) ! -name 'mtl_*' -delete
	echo "post-build: GPU firmware kept: $(find "${gpu_firmware}" \( -type f -o -type l \) -printf '%f\n' | sort | tr '\n' ' ')"
fi

# --- Files the overlay no longer has ---------------------------------------------
# Buildroot copies the overlay over the target tree and never removes
# from it: a file taken out of the repository would stay in every image
# built from the same output directory. Each file retired from the
# overlay is named here.
#   dawos-vm-modules, dawos-vm.conf: KVM is built into the kernel since
#   step P1.0, there are no modules to load or to keep from loading.
rm -f "${target_dir}/usr/bin/dawos-vm-modules" "${target_dir}/etc/modprobe.d/dawos-vm.conf"

# --- Kernel modules: only those of the kernel just built ---------------------------
# For the same reason the modules of an earlier kernel version, and the
# modules of this one that have since become part of the kernel itself,
# stay in the target tree. A module file whose code is already in the
# kernel is loaded by udev at boot and fails. Keep what the last kernel
# build lists (modules.order), drop the rest, and index again.
: "${BUILD_DIR:?BUILD_DIR is not set (post-build.sh is run by Buildroot)}"
: "${BR2_CONFIG:?BR2_CONFIG is not set (post-build.sh is run by Buildroot)}"
kernel_version=$(sed -n 's/^BR2_LINUX_KERNEL_VERSION="\(.*\)"$/\1/p' "${BR2_CONFIG}")
kernel_build=${BUILD_DIR}/linux-${kernel_version}
if [[ -n ${kernel_version} && -r ${kernel_build}/include/config/kernel.release ]]; then
	kernel_release=$(< "${kernel_build}/include/config/kernel.release")
	modules_root=${target_dir}/lib/modules
	if [[ -d ${modules_root} ]]; then
		while IFS= read -r other; do
			echo "post-build: removing the modules of another kernel: ${other##*/}"
			rm -rf "${other}"
		done < <(find "${modules_root}" -mindepth 1 -maxdepth 1 -type d ! -name "${kernel_release}")
	fi
	modules_dir=${modules_root}/${kernel_release}
	if [[ -d ${modules_dir}/kernel ]]; then
		wanted=$(sed 's/\.o$//' "${kernel_build}/modules.order" 2> /dev/null || true)
		removed=0
		while IFS= read -r module; do
			name=${module#"${modules_dir}/kernel/"}
			name=${name%%.ko*}
			if ! grep -qxF "${name}" <<< "${wanted}"; then
				rm -f "${module}"
				removed=$((removed + 1))
			fi
		done < <(find "${modules_dir}/kernel" -type f -name '*.ko*')
		find "${modules_dir}/kernel" -depth -type d -empty -delete
		if ((removed > 0)); then
			echo "post-build: removed ${removed} module files that the kernel build no longer produces"
			"${HOST_DIR:?HOST_DIR is not set}/sbin/depmod" -a -b "${target_dir}" "${kernel_release}"
		fi
	fi
fi
