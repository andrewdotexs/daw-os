#!/usr/bin/env bash
# dawOS post-build hook: runs after all packages are installed in the
# target tree and before the filesystem images are created.
#
# Called by Buildroot as:  post-build.sh <TARGET_DIR> <variant>
#
# It only stamps the image identity; everything else belongs either to a
# package or to the rootfs overlay.

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
