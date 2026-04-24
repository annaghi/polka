.DEFAULT_GOAL := help
.PHONY: help test test-integration fix update

help:  ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make <target>\n\nTargets:\n"} \
	     /^[a-zA-Z_-]+:.*##/ {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}' \
	     $(MAKEFILE_LIST)

test:  ## Run all polka tests
	cargo test -p polka

test-integration:  ## Run integration tests only
	cargo test -p polka --test integration

fix:  ## Run pre-commit with auto-fix
	prek run --all-files

update:  ## Update pre-commit hooks
	prek auto-update
