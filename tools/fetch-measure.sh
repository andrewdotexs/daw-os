#!/usr/bin/env bash
# Copy the measurement results from a dawOS machine into the repository.
#
# Usage: fetch-measure.sh HOST [DESTINATION]
#
# Results live in /var/lib/dawos/measure/ on the machine, which is in
# memory until the "state" partition is mounted (step P0.5): fetch them
# before rebooting it. The default destination is docs/misure/fase-0/raw/.

set -euo pipefail

host=${1:-}
[[ -n ${host} && ${host} != -* ]] || {
	echo "usage: ${0##*/} HOST [DESTINATION]" >&2
	exit 1
}

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
dest=${2:-${repo_dir}/docs/misure/fase-0/raw}
remote_dir=/var/lib/dawos/measure

mkdir -p "${dest}"

# "/." copies the content of the directory, not the directory itself.
if ! scp -r -o ConnectTimeout=10 "root@${host}:${remote_dir}/." "${dest}/"; then
	echo "fetch-measure: nothing copied from ${host}:${remote_dir}" >&2
	exit 1
fi

echo "Results copied to ${dest}:"
find "${dest}" -mindepth 1 -maxdepth 1 -type d -printf '  %f\n' | sort
