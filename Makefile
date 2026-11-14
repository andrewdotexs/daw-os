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
# Number of parallel compile jobs. Empty means Buildroot's default, one
# more than the CPUs it sees. Lower it when RAM is short, about one job
# per GiB: "make image JOBS=6". "make check-env" suggests a value.
JOBS ?=

# markdownlint-cli2 release used by "make lint" and by the lint workflow.
# 0.17.2 is the last line that runs on Node.js 18, the version packaged by
# Ubuntu 24.04; newer releases need Node.js 20 or 22.
MARKDOWNLINT_VERSION := 0.17.2

# Every shell script of the project, wherever it lives.
OVERLAY_DIR := $(EXT_DIR)/board/dawos/x86_64/rootfs-overlay
SHELL_SOURCES := $(wildcard tools/*.sh) \
	$(wildcard $(EXT_DIR)/board/dawos/x86_64/*.sh) \
	$(wildcard $(OVERLAY_DIR)/usr/libexec/dawos/*) \
	$(wildcard $(OVERLAY_DIR)/usr/bin/*) \
	$(wildcard $(OVERLAY_DIR)/etc/profile.d/*.sh)

# Buildroot refuses paths with spaces, and so do we, early and clearly.
ifneq ($(word 2,$(TOP)),)
$(error The repository path contains spaces: move it, e.g. to ~/daw-os)
endif

# One place for the Buildroot invocation: out-of-tree build, our external
# tree, download cache kept outside the output so "make distclean" keeps it.
BR_MAKE = $(MAKE) -C $(BR_DIR) BR2_EXTERNAL=$(EXT_DIR) O=$(O) BR2_DL_DIR=$(DL) \
	$(if $(JOBS),BR2_JLEVEL=$(JOBS))

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

# The Buildroot configuration follows the defconfig: it is (re)created when
# missing and whenever the defconfig is newer, for example after "git pull".
# Local "make menuconfig" changes are lost at that point: save them first.
$(O)/.config: $(EXT_DIR)/configs/$(DEFCONFIG) | $(BR_DIR)/Makefile
	$(BR_MAKE) $(DEFCONFIG)

.PHONY: config
config: | $(BR_DIR)/Makefile ## (Re)apply the defconfig, discarding local menuconfig changes
	$(BR_MAKE) $(DEFCONFIG)

.PHONY: image
image: $(O)/.config ## Build the whole image (output/images/disk.img); JOBS=N limits parallelism
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

.PHONY: qemu-installer
qemu-installer: ## Rehearse the installation: boot the stick image with a blank NVMe disk
	@tools/run-qemu.sh --installer

.PHONY: qemu-target
qemu-target: ## Boot the disk installed by the rehearsal (exit: Ctrl-A then X)
	@tools/run-qemu.sh --target

.PHONY: qemu-target-test
qemu-target-test: ## Boot the disk installed by the rehearsal and check the boot report
	@tools/run-qemu.sh --target --test

.PHONY: remote-test
remote-test: ## Check a real machine over SSH: make remote-test HOST=<address>
	@tools/remote-check.sh $(HOST)

.PHONY: measure-prepare
measure-prepare: ## After every boot of a machine: copy the tools, keep results on disk: make measure-prepare HOST=<address>
	@tools/prepare-measure.sh $(HOST)

.PHONY: measure-fetch
measure-fetch: ## Copy measurement results from a machine: make measure-fetch HOST=<address>
	@tools/fetch-measure.sh $(HOST)

.PHONY: lint
lint: ## Run shellcheck and markdownlint
	shellcheck $(SHELL_SOURCES)
	npx --yes markdownlint-cli2@$(MARKDOWNLINT_VERSION)

# Pass any other target straight to Buildroot, e.g. "make br-linux-rebuild".
br-%: $(O)/.config
	$(BR_MAKE) $*

.PHONY: clean
clean: | $(BR_DIR)/Makefile ## Remove build results, keep configuration and downloads
	$(BR_MAKE) clean

.PHONY: distclean
distclean: ## Remove the whole output tree (downloads in dl/ are kept)
	rm -rf $(O)
