.PHONY: check test host flutter-deps

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

host:
	./scripts/build_host_core.sh

flutter-deps:
	cd apps/nexus_app && flutter pub get
