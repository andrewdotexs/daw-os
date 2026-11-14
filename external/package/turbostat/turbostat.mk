################################################################################
#
# turbostat
#
################################################################################

# No sources of its own: built in place from the kernel tree, like the
# tools of Buildroot's linux-tools package.
TURBOSTAT_VERSION = kernel
TURBOSTAT_SOURCE =
TURBOSTAT_LICENSE = GPL-2.0
TURBOSTAT_LICENSE_FILES =
TURBOSTAT_DEPENDENCIES = linux libcap

TURBOSTAT_DIR_IN_KERNEL = $(LINUX_DIR)/tools/power/x86/turbostat

define TURBOSTAT_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(MAKE) -C $(TURBOSTAT_DIR_IN_KERNEL) \
		CC="$(TARGET_CC)" \
		CFLAGS="$(TARGET_CFLAGS)" \
		LDFLAGS="$(TARGET_LDFLAGS)" \
		turbostat
endef

define TURBOSTAT_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(TURBOSTAT_DIR_IN_KERNEL)/turbostat \
		$(TARGET_DIR)/usr/bin/turbostat
endef

$(eval $(generic-package))
