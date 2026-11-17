################################################################################
#
# dawos-dspbench
#
################################################################################

# One C file kept in this repository, next to this makefile. Buildroot
# copies it once and does not look at it again: raise the version at
# every change of the source, so that it is built anew.
DAWOS_DSPBENCH_VERSION = 2
DAWOS_DSPBENCH_SITE = $(BR2_EXTERNAL_DAWOS_PATH)/package/dawos-dspbench/src
DAWOS_DSPBENCH_SITE_METHOD = local
DAWOS_DSPBENCH_LICENSE = GPL-3.0-or-later

# AVX2 and FMA are asked for explicitly: the image is built for a generic
# x86-64 processor, while this program is only ever run where it has
# checked that both exist.
define DAWOS_DSPBENCH_BUILD_CMDS
	$(TARGET_CC) $(TARGET_CFLAGS) -O2 -mavx2 -mfma -Wall -Wextra \
		-o $(@D)/dspbench $(@D)/dspbench.c $(TARGET_LDFLAGS) -lm
endef

define DAWOS_DSPBENCH_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/dspbench $(TARGET_DIR)/usr/libexec/dawos/dspbench
endef

$(eval $(generic-package))
