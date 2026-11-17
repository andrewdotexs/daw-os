#!/usr/bin/env bash
# Look for network addresses that must stay out of the repository.
#
# Usage: check-private.sh [--staged] [PATH...]
#
# The address of the reference machine, those of the network it sits on
# and hardware addresses (MAC) belong in local/ or keys/, never in a
# committed file: a measurement copied from the machine can carry one
# without anyone noticing. This looks for private IPv4 addresses, IPv6
# link-local addresses (which often contain a MAC) and MAC addresses.
#
# Without PATH: the documents and the sources. With --staged: the files
# staged for the next commit, which is how the pre-commit hook in
# tools/hooks calls it. Exit status 1 if anything is found.
#
# The addresses of QEMU's built-in network (10.0.2.x) are the same on
# every machine and are allowed.

set -euo pipefail

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "${repo_dir}"

pattern='192\.168\.[0-9]+\.[0-9]+|(^|[^0-9.])10\.[0-9]+\.[0-9]+\.[0-9]+([^0-9.]|$)|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]+\.[0-9]+|fe80:|([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}'
allowed='(^|[^0-9.])10\.0\.2\.[0-9]+([^0-9.]|$)'

files=()
if [[ ${1:-} == --staged ]]; then
	while IFS= read -r -d '' file; do
		[[ -f ${file} ]] && files+=("${file}")
	done < <(git diff --cached --name-only --diff-filter=ACM -z)
elif [[ $# -gt 0 ]]; then
	files=("$@")
else
	files=(docs external tools Makefile README.md)
fi
[[ ${#files[@]} -gt 0 ]] || exit 0

# This script and its hook name the patterns themselves.
found=$(grep -rnIE "${pattern}" -- "${files[@]}" 2> /dev/null |
	grep -vE "${allowed}" |
	grep -vE '^tools/(check-private\.sh|hooks/pre-commit):' || true)
if [[ -n ${found} ]]; then
	echo "check-private: network or hardware addresses found:" >&2
	echo "${found}" | cut -c 1-200 >&2
	echo "check-private: remove them (they belong in local/ or keys/), then try again." >&2
	exit 1
fi
