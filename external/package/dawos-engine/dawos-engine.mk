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

# The profile of crates/Cargo.toml, said again here: Buildroot gives
# every cargo package its own (opt-level 2, no overflow checks) through
# the environment, which wins over the file. The engine is measured as
# its sources say it is built: opt-level 3, and an arithmetic overflow
# stops it instead of going by unseen (choice of the user of 25 November
# 2026; until then the image had Buildroot's profile).
DAWOS_ENGINE_PROFILE = \
	CARGO_PROFILE_RELEASE_OPT_LEVEL=3 \
	CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true

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
		$(DAWOS_ENGINE_PROFILE) \
		cargo --config cargo/vendored.toml build --offline --locked --release \
			--package dawos-engine
endef

# With the engine, the graphs that come with its sources: the ones the
# tests and the measures of step P1.7 run (dawos-engine graph FILE says
# what each live thread would do with one).
define DAWOS_ENGINE_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 \
		$(@D)/target/$(RUSTC_TARGET_NAME)/release/dawos-engine \
		$(TARGET_DIR)/usr/bin/dawos-engine
	$(INSTALL) -d $(TARGET_DIR)/usr/share/dawos/graphs
	$(INSTALL) -m 0644 $(@D)/dawos-graph/graphs/*.graph \
		$(TARGET_DIR)/usr/share/dawos/graphs/
endef

$(eval $(generic-package))
