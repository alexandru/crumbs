.PHONY: test build docker-build-local update-dependencies

test:
	cd backend && cargo test --workspace --all-features --locked

build:
	cd backend && cargo build --locked --release --package crumbs-main --bin crumbs

docker-build-local:
	docker build -t crumbs ./backend

update-dependencies:
	cargo install cargo-edit && cd backend && cargo upgrade
