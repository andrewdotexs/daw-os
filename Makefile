# dawOS — top-level entry points.
#
# This Makefile only drives Buildroot and the helper scripts; it holds no
# build logic of its own. Run "make help" for the list of targets.

SHELL := /bin/bash
.DEFAULT_GOAL := help

TOP := $(CURDIR)
BR_DIR := $(TOP)/buildroot
EXT_DIR := $(TOP)/external

# Overridable from the command line or the environment.
O ?= $(TOP)/output
DL ?= $(TOP)/dl
DEFCONFIG ?= dawos_x86_64_dev_defconfig

# Buildroot refuses paths with spaces, and so do we, early and clearly.
ifneq ($(word 2,$(TOP)),)
$(error The repository path contains spaces: move it, e.g. to ~/daw-os)
endif

# One place for the Buildroot invocation: out-of-tree build, our external
# tree, download cache kept outside the output so "make distclean" keeps it.
BR_MAKE = $(MAKE) -C $(BR_DIR) BR2_EXTERNAL=$(EXT_DIR) O=$(O) BR2_DL_DIR=$(DL)

.PHONY: help
help: ## Show this help
	@awk 'BEGIN { FS = ":.*## " } /^[a-zA-Z0-9_%-]+:.*## / { printf "  %-18s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

.PHONY: check-env
check-env: ## Check that this machine can build and run dawOS (step P0.1)
	@tools/check-env.sh

# The submodule is not fetched by a plain "git clone".
$(BR_DIR)/Makefile:
	@echo "Buildroot submodule missing: run 'git submodule update --init'" >&2
	@exit 1

# First configuration: created from the defconfig if it does not exist.
$(O)/.config: | $(BR_DIR)/Makefile
	$(BR_MAKE) $(DEFCONFIG)

.PHONY: config
config: | $(BR_DIR)/Makefile ## (Re)apply the defconfig, discarding local menuconfig changes
	$(BR_MAKE) $(DEFCONFIG)

.PHONY: image
image: $(O)/.config ## Build the whole image (output/images/disk.img)
	$(BR_MAKE) all

.PHONY: menuconfig
menuconfig: $(O)/.config ## Edit the Buildroot configuration
	$(BR_MAKE) menuconfig

.PHONY: linux-menuconfig
linux-menuconfig: $(O)/.config ## Edit the kernel configuration (then update the fragments by hand)
	$(BR_MAKE) linux-menuconfig

# Buildroot writes a minimal defconfig without comments. Ours is commented
# and maintained by hand, so the result goes to a side file to be merged.
.PHONY: savedefconfig
savedefconfig: $(O)/.config ## Write the current configuration to output/defconfig.saved
	$(BR_MAKE) savedefconfig BR2_DEFCONFIG=$(O)/defconfig.saved
	@echo "Compare with: diff <(grep -v '^#' external/configs/$(DEFCONFIG) | grep .) $(O)/defconfig.saved"

.PHONY: qemu
qemu: ## Boot the image in QEMU with a serial console (exit: Ctrl-A then X)
	@tools/run-qemu.sh

.PHONY: qemu-test
qemu-test: ## Boot the image in QEMU and check the boot report (PASS/FAIL)
	@tools/run-qemu.sh --test

.PHONY: lint
lint: ## Run shellcheck and markdownlint
	shellcheck tools/*.sh external/board/dawos/x86_64/*.sh \
		external/board/dawos/x86_64/rootfs-overlay/usr/libexec/dawos/*
	npx --yes markdownlint-cli2@0.24.1

# Pass any other target straight to Buildroot, e.g. "make br-linux-rebuild".
br-%: $(O)/.config
	$(BR_MAKE) $*

.PHONY: clean
clean: | $(BR_DIR)/Makefile ## Remove build results, keep configuration and downloads
	$(BR_MAKE) clean

.PHONY: distclean
distclean: ## Remove the whole output tree (downloads in dl/ are kept)
	rm -rf $(O)
