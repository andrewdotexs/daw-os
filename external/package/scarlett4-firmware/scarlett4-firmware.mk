################################################################################
#
# scarlett4-firmware
#
################################################################################

# The commit of the tag 1.1 (19 July 2026): firmware 2778 for the three
# Scarlett of the fourth generation. A commit and not the tag, which can
# be moved; fetched with git for the reason given in fcp-support.mk.
SCARLETT4_FIRMWARE_VERSION = 183d5ec745bcdb19ac5e723c1f4012fa1bbd549e
SCARLETT4_FIRMWARE_SITE = https://github.com/geoffreybennett/scarlett4-firmware.git
SCARLETT4_FIRMWARE_SITE_METHOD = git
SCARLETT4_FIRMWARE_LICENSE = Focusrite firmware licence (redistribution unmodified, for Focusrite products only)
SCARLETT4_FIRMWARE_LICENSE_FILES = LICENSE.Focusrite

# The interface of the project only: the 18i20 (USB 1235:821d). fcp-tool
# reads every .bin file of the directory and takes the newest one made
# for the interface it finds.
SCARLETT4_FIRMWARE_FILES = firmware/scarlett4-1235-821d-2778.bin

define SCARLETT4_FIRMWARE_INSTALL_TARGET_CMDS
	$(INSTALL) -d $(TARGET_DIR)/usr/lib/firmware/scarlett4
	$(INSTALL) -m 0644 $(addprefix $(@D)/,$(SCARLETT4_FIRMWARE_FILES)) \
		$(TARGET_DIR)/usr/lib/firmware/scarlett4/
endef

$(eval $(generic-package))
