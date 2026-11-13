# dawOS BR2_EXTERNAL tree: pull in every package makefile shipped here.
# The wildcard is sorted so that the include order is deterministic.
include $(sort $(wildcard $(BR2_EXTERNAL_DAWOS_PATH)/package/*/*.mk))
