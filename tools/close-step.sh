#!/usr/bin/env bash
# Close a step: merge its accepted state into main and tag it.
#
# Usage: close-step.sh TAG COMMIT TITLE [NOTE]
#
#   TAG     tag of the step, like p0.3 or p0.4a
#   COMMIT  the commit that was accepted (hash, or a branch name)
#   TITLE   one line, like "P0.3: Linux 6.18 PREEMPT_RT kernel"
#   NOTE    how and when the step was accepted (optional)
#
# The result on main is one merge commit per step, with an annotated tag
# on it: "git log --first-parent main" lists the steps, and
# "git checkout p0.3" goes back to a state that is known to work.
#
# The merge is made in a temporary worktree, so the files of the current
# checkout are not touched: no branch switch, no timestamps changed, no
# needless rebuild of the image afterwards.

set -euo pipefail

usage() {
	sed -n '2,10p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
	exit 1
}

die() {
	echo "close-step: $*" >&2
	exit 1
}

(($# >= 3)) || usage
tag=$1
commit=$2
title=$3
note=${4:-}

[[ ${tag} =~ ^p[0-9]+\.[0-9]+[a-z]?$ ]] || die "tag '${tag}' does not look like p0.3 or p0.4a"

repo_dir=$(git rev-parse --show-toplevel)
cd "${repo_dir}"

# A merge commit and an annotated tag both need an author: say so now,
# in plain words, instead of failing halfway through.
if ! git var GIT_COMMITTER_IDENT > /dev/null 2>&1; then
	die "git does not know who you are. Set your identity once with:
  git config --global user.name  \"Your Name\"
  git config --global user.email \"you@example.com\"
then run this command again."
fi

git fetch --quiet --tags origin

sha=$(git rev-parse --verify --quiet "${commit}^{commit}") || die "unknown commit '${commit}'"
git rev-parse --verify --quiet "refs/tags/${tag}" > /dev/null && die "tag ${tag} already exists"
git rev-parse --verify --quiet origin/main > /dev/null || die "origin/main not found"
if git merge-base --is-ancestor "${sha}" origin/main; then
	die "commit ${sha:0:7} is already part of main"
fi

message="Merge step ${title}"
tag_message=${title}
if [[ -n ${note} ]]; then
	message+=$'\n\n'"${note}"
	tag_message+=$'\n\n'"${note}"
fi

# --- Merge in a throw-away worktree -------------------------------------------
work_dir=$(mktemp -d)
cleanup() {
	git worktree remove --force "${work_dir}" 2> /dev/null || true
	rm -rf "${work_dir}"
}
trap cleanup EXIT

git worktree add --quiet --detach "${work_dir}" origin/main
if ! merge_output=$(git -C "${work_dir}" merge --no-ff -m "${message}" "${sha}" 2>&1); then
	echo "${merge_output}" >&2
	die "the merge did not complete (see git's message above). Nothing was pushed. If it reports conflicts, main was changed outside the working branch."
fi
merge_sha=$(git -C "${work_dir}" rev-parse HEAD)

# The merged tree must be exactly the accepted one: main only ever
# receives what was accepted on the working branch.
if [[ $(git rev-parse "${merge_sha}^{tree}") != $(git rev-parse "${sha}^{tree}") ]]; then
	die "the merged tree differs from ${sha:0:7}: main has content the working branch lacks"
fi

git tag -a "${tag}" -m "${tag_message}" "${merge_sha}"

# --- Push main and the tag together ---------------------------------------------
if ! git push --atomic origin "${merge_sha}:refs/heads/main" "refs/tags/${tag}"; then
	git tag -d "${tag}" > /dev/null
	die "push failed: nothing was changed on the remote, and the local tag was removed"
fi

# Keep a local "main" in step, unless it is the branch checked out here.
if [[ $(git symbolic-ref --quiet --short HEAD || true) != main ]]; then
	git branch --quiet --force main "${merge_sha}"
fi

echo "Step closed: ${tag} -> ${merge_sha:0:7} (${title})"
echo "main now contains the steps:"
git log --first-parent --format='  %h %s' "${merge_sha}" | sed '$d'
