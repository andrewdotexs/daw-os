# dawOS BR2_EXTERNAL tree: pull in every package makefile shipped here.
# The wildcard is sorted so that the include order is deterministic.
include $(sort $(wildcard $(BR2_EXTERNAL_DAWOS_PATH)/package/*/*.mk))

# The kernel records who built it and on which machine, and prints it in
# the first line of every boot log ("Linux version ... (user@host)").
# A fixed name instead: the log of a machine is copied into measurement
# files, and the name of a person's PC has no place there.
LINUX_MAKE_ENV += KBUILD_BUILD_USER=dawos KBUILD_BUILD_HOST=build
