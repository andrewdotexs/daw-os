################################################################################
#
# fcp-support
#
################################################################################

# A commit, not a release: four commits after the tag 1.0beta1 (26 August
# 2026), the last of 5 November 2026. Fetched with git, because the
# archive of a commit that GitHub makes on request is not reachable from
# every place this image is built in; Buildroot packs the clone itself,
# always in the same way, and the hash file is of that archive.
FCP_SUPPORT_VERSION = 02be3c9081658af9c5b1246294a054d5eb24ef60
FCP_SUPPORT_SITE = https://github.com/geoffreybennett/fcp-support.git
FCP_SUPPORT_SITE_METHOD = git
FCP_SUPPORT_LICENSE = GPL-3.0-or-later
FCP_SUPPORT_LICENSE_FILES = COPYING
FCP_SUPPORT_DEPENDENCIES = host-pkgconf alsa-lib json-c openssl systemd zlib

# What the programs say when asked for their version. The makefile of the
# package asks git, and here git would answer with the tag of the
# repository the build directory is in: this one. So it is given, as
# `git describe --abbrev=4 --tags | sed 's/-/./g'` prints it in a clone
# of the commit above. To be changed together with it.
FCP_SUPPORT_SAYS_VERSION = 1.0beta1.4.g02be

# PREFIX is compiled into fcp-server (where it looks for the maps of the
# interfaces) and written in the unit file. CFLAGS and LDFLAGS go through
# the environment, not the command line: the makefile adds its own to
# them, and a variable of the command line cannot be added to.
FCP_SUPPORT_MAKE_ENV = \
	$(TARGET_MAKE_ENV) \
	CFLAGS="$(TARGET_CFLAGS)" \
	LDFLAGS="$(TARGET_LDFLAGS)"

FCP_SUPPORT_MAKE_OPTS = \
	CC="$(TARGET_CC)" \
	PKG_CONFIG="$(PKG_CONFIG_HOST_BINARY)" \
	PREFIX=/usr \
	VERSION="$(FCP_SUPPORT_SAYS_VERSION)"

define FCP_SUPPORT_BUILD_CMDS
	$(FCP_SUPPORT_MAKE_ENV) $(MAKE) -C $(@D) $(FCP_SUPPORT_MAKE_OPTS) all
endef

# Installs the two programs in /usr/bin, the unit fcp-server@.service,
# the line of tmpfiles.d that makes /run/fcp-server, the udev rule that
# starts the unit when an interface of Focusrite appears, and the maps
# of the interfaces in /usr/share/fcp-server.
define FCP_SUPPORT_INSTALL_TARGET_CMDS
	$(FCP_SUPPORT_MAKE_ENV) $(MAKE) -C $(@D) $(FCP_SUPPORT_MAKE_OPTS) \
		DESTDIR="$(TARGET_DIR)" install
endef

$(eval $(generic-package))
