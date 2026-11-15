#!/usr/bin/env bash
# Notice when the options of an already built Buildroot package changed.
#
# Usage: package-options.sh changed OUTPUT_DIR PACKAGE OPTION_PREFIX
#        package-options.sh record  OUTPUT_DIR PACKAGE OPTION_PREFIX
#
# Buildroot builds a package once. If its options change later (a feature
# switched on in the defconfig, for instance) it is NOT built again: the
# image silently keeps the old build, without the new feature. The
# documented remedy is "make <package>-dirclean" by hand, which is easy
# to forget and whose absence shows up far from its cause.
#
# "record" stores the options of PACKAGE (the lines of .config starting
# with OPTION_PREFIX) after a successful build; "changed" exits 0 when the
# package is already built and its options differ from the stored ones,
# or were never stored. The Makefile then cleans the package so that it
# is rebuilt.

set -euo pipefail

action=${1:?usage: package-options.sh changed|record OUTPUT_DIR PACKAGE OPTION_PREFIX}
output_dir=${2:?missing OUTPUT_DIR}
package=${3:?missing PACKAGE}
prefix=${4:?missing OPTION_PREFIX}

config=${output_dir}/.config
stamp=${output_dir}/build/.dawos-options-${package}

current() {
	# Enabled options and explicit "is not set" lines both count.
	grep -E "^(# )?${prefix}" "${config}" | sort
}

case ${action} in
record)
	[[ -f ${config} && -d ${output_dir}/build ]] || exit 0
	current > "${stamp}"
	;;
changed)
	[[ -f ${config} ]] || exit 1
	# Not built yet: Buildroot will build it with the current options.
	compgen -G "${output_dir}/build/${package}-[0-9]*" > /dev/null || exit 1
	[[ -f ${stamp} ]] || exit 0
	if diff -q <(current) "${stamp}" > /dev/null; then
		exit 1
	fi
	exit 0
	;;
*)
	echo "package-options.sh: unknown action '${action}'" >&2
	exit 2
	;;
esac
