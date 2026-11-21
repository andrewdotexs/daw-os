################################################################################
#
# dawos-engine
#
################################################################################

# The workspace in crates/ of this repository. Buildroot copies it into
# its build directory once and does not look at it again by itself:
# "make image" asks for the copy and the build to be redone every time
# (see the Makefile), and cargo then rebuilds only what changed.
DAWOS_ENGINE_VERSION = 0.1.0
DAWOS_ENGINE_SITE = $(BR2_EXTERNAL_DAWOS_PATH)/../crates
DAWOS_ENGINE_SITE_METHOD = local
DAWOS_ENGINE_LICENSE = GPL-3.0-or-later
# alsa-lib: the ALSA back end links against it (crate "alsa"), and the
# build script of the crate "alsa-sys" asks pkg-config where it is.
DAWOS_ENGINE_DEPENDENCIES = host-rustc host-pkgconf alsa-lib
# What a build on the development PC leaves in crates/ and has no place
# in the image build. The leading slash matters: without it rsync would
# also leave out every directory called "target" inside vendor/, and the
# crate "cc" has one.
DAWOS_ENGINE_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = --exclude /target

# The environment Buildroot prepares for cargo packages: the Rust target
# of the image, its linker, optimised code. "--offline --locked" and the
# vendored sources: this step never reaches for the network, and builds
# exactly the versions of Cargo.lock.
define DAWOS_ENGINE_BUILD_CMDS
	test -d $(@D)/vendor || { \
		echo "dawos-engine: crates/vendor is missing: run 'make engine-vendor' first" >&2; \
		exit 1; }
	cd $(@D) && \
		$(TARGET_MAKE_ENV) $(TARGET_CONFIGURE_OPTS) $(PKG_CARGO_ENV) \
		cargo --config cargo/vendored.toml build --offline --locked --release \
			--package dawos-engine
endef

define DAWOS_ENGINE_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 \
		$(@D)/target/$(RUSTC_TARGET_NAME)/release/dawos-engine \
		$(TARGET_DIR)/usr/bin/dawos-engine
endef

$(eval $(generic-package))
