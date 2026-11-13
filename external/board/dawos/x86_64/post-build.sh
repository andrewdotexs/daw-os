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
#   keys/   SSH host key of dev images, generated here on first use
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

# Exact source revision, for bug reports and measurement reports. Falls
# back gracefully when the tree is not a git checkout (release tarball).
build_id=$(git -C "${repo_dir}" describe --always --dirty --abbrev=12 2>/dev/null || echo unknown)

# os-release(5). IMAGE_ID and IMAGE_VERSION are the fields systemd-sysupdate
# and the boot report read; VARIANT_ID tells "dev" from "prod".
install -d -m 0755 "${target_dir}/usr/lib"
cat > "${target_dir}/usr/lib/os-release" <<OSRELEASE
NAME="dawOS"
ID=dawos
PRETTY_NAME="dawOS ${version} (${variant})"
VERSION="${version}"
VERSION_ID=${version}
VARIANT_ID=${variant}
IMAGE_ID=dawos
IMAGE_VERSION=${version}
BUILD_ID=${build_id}
OSRELEASE
ln -sfn ../usr/lib/os-release "${target_dir}/etc/os-release"

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
	echo "post-build: static network configuration for driver '${driver}' installed"
else
	echo "post-build: no ${net_conf}: DHCP on every wired interface"
fi
