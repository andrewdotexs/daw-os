# shellcheck shell=sh
# Sourced by /etc/profile. Shows a hint at login when the system was
# started from the installation medium.
case " $(cat /proc/cmdline) " in
*" dawos.installer "*)
	cat <<'HINT'

  This is the dawOS installation medium.

    dawos-install --list     show the disks of this machine
    dawos-install --help     how to install dawOS on the internal disk

  Nothing is written unless you ask for it with --write and confirm.

HINT
	;;
esac
