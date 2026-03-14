.PHONY: example tree
.PHONY: fix
.PHONY: update

example:
	cargo run --example main -- examples/block_attrs/atx_heading.md --output tmp/output.html --icon-dir assets/.icons

tree:
	cargo run --example main -- --tree examples/block_attrs/atx_heading.md

fix:
	@echo "Running pre-commit with auto-fix..."
	prek run --all-files

update:
	@echo "Updating pre-commit hooks..."
	prek auto-update

test:
	cargo test --lib
	cargo test --test main
