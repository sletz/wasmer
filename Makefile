.PHONY: spectests emtests clean build install lint precommit docs examples

# Generate files
generate-spectests:
	WASMER_RUNTIME_GENERATE_SPECTESTS=1 cargo build -p wasmer-runtime-core --release \
	&& echo "formatting" \
	&& cargo fmt

generate-emtests:
	WASM_EMSCRIPTEN_GENERATE_EMTESTS=1 cargo build -p wasmer-emscripten-tests --release \
	&& echo "formatting" \
	&& cargo fmt

generate-wasitests: wasitests-setup
	WASM_WASI_GENERATE_WASITESTS=1 cargo build -p wasmer-wasi-tests --release -vv \
	&& echo "formatting" \
	&& cargo fmt

spectests-generate: generate-spectests
emtests-generate: generate-emtests
wasitests-generate: generate-wasitests

generate: generate-spectests generate-emtests generate-wasitests


# Spectests
spectests-singlepass:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features singlepass -- --nocapture

spectests-cranelift:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features clif -- --nocapture

spectests-llvm:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features llvm -- --nocapture

spectests: spectests-singlepass spectests-cranelift spectests-llvm


# Emscripten tests
emtests-singlepass:
	cargo test --manifest-path lib/emscripten-tests/Cargo.toml --release --features singlepass -- --test-threads=1

emtests-cranelift:
	cargo test --manifest-path lib/emscripten-tests/Cargo.toml --release --features clif -- --test-threads=1

emtests-llvm:
	cargo test --manifest-path lib/emscripten-tests/Cargo.toml --release --features llvm -- --test-threads=1

emtests-unit:
	cargo test --manifest-path lib/emscripten/Cargo.toml --release

emtests: emtests-unit emtests-singlepass emtests-cranelift emtests-llvm


# Middleware tests
middleware-singlepass:
	cargo test --manifest-path lib/middleware-common-tests/Cargo.toml --release --features singlepass

middleware-cranelift:
	cargo test --manifest-path lib/middleware-common-tests/Cargo.toml --release --features clif

middleware-llvm:
	cargo test --manifest-path lib/middleware-common-tests/Cargo.toml --release --features llvm

middleware: middleware-singlepass middleware-cranelift middleware-llvm


# Wasitests
wasitests-setup:
	rm -rf lib/wasi-tests/wasitests/test_fs/temp
	mkdir -p lib/wasi-tests/wasitests/test_fs/temp

wasitests-singlepass: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features singlepass -- --test-threads=1

wasitests-cranelift: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features clif -- --test-threads=1 --nocapture

wasitests-llvm: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features llvm -- --test-threads=1

wasitests-unit: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features clif -- --test-threads=1 --nocapture
	cargo test --manifest-path lib/wasi/Cargo.toml --release

wasitests: wasitests-unit wasitests-singlepass wasitests-cranelift wasitests-llvm


# Backends
singlepass: spectests-singlepass emtests-singlepass middleware-singlepass wasitests-singlepass
	cargo test -p wasmer-singlepass-backend --release

cranelift: spectests-cranelift emtests-cranelift middleware-cranelift wasitests-cranelift
	cargo test -p wasmer-clif-backend --release

llvm: spectests-llvm emtests-llvm wasitests-llvm
	cargo test -p wasmer-llvm-backend --release


# All tests
capi:
	cargo build --release
	cargo build -p wasmer-runtime-c-api --release

test-capi: capi
	cargo test -p wasmer-runtime-c-api --release

test-rest:
	cargo test --release --all --exclude wasmer-runtime-c-api --exclude wasmer-emscripten --exclude wasmer-spectests --exclude wasmer-wasi --exclude wasmer-middleware-common --exclude wasmer-middleware-common-tests --exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-wasi-tests --exclude wasmer-emscripten-tests

circleci-clean:
	@if [ ! -z "${CIRCLE_JOB}" ]; then rm -f /home/circleci/project/target/debug/deps/libcranelift_wasm* && rm -f /Users/distiller/project/target/debug/deps/libcranelift_wasm*; fi;

test: spectests emtests middleware wasitests circleci-clean test-rest


# Integration tests
integration-tests: release-clif examples
	echo "Running Integration Tests"
	./integration_tests/lua/test.sh
	./integration_tests/nginx/test.sh
	./integration_tests/cowsay/test.sh

examples:
	cargo run --example plugin
	cargo run --example callback


# Utils
lint:
	cargo fmt --all -- --check

precommit: lint test

debug:
	cargo build --release --features backend-singlepass,debug,trace

install:
	cargo install --path .

# Checks
check-bench-singlepass:
	cargo bench --all --no-run --no-default-features --features "backend-singlepass" \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
check-bench-clif:
	cargo bench --all --no-run --no-default-features --features "backend-cranelift" \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader \
	--exclude wasmer-middleware-common-tests
check-bench-llvm:
	cargo bench --all --no-run --no-default-features --features "backend-llvm" \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

check-bench: check-bench-singlepass check-bench-llvm

check: check-bench
	cargo check --release --features backend-singlepass,backend-llvm,loader-kernel,debug

# Release
release:
	cargo build --release --features backend-singlepass,backend-llvm,loader-kernel

# Only one backend (cranelift)
release-clif:
	# If you are on macOS, you will need mingw-w64 for cross compiling to Windows
	# brew install mingw-w64
	cargo build --release

release-singlepass:
	cargo build --release --features backend-singlepass

release-llvm:
	cargo build --release --features backend-llvm

bench-singlepass:
	cargo bench --all --no-default-features --features "backend-singlepass" \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
bench-clif:
	cargo bench --all --no-default-features --features "backend-cranelift" \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader \
	--exclude wasmer-middleware-common-tests
bench-llvm:
	cargo bench --all --no-default-features --features "backend-llvm" \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

# Build utils
build-install:
	mkdir -p ./install/bin
	cp ./wapm-cli/target/release/wapm ./install/bin/
	cp ./target/release/wasmer ./install/bin/
	tar -C ./install -zcvf wasmer.tar.gz bin/wapm bin/wasmer

# For installing the contents locally
do-install:
	tar -C ~/.wasmer -zxvf wasmer.tar.gz

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/

# cargo install cargo-deps
# must install graphviz for `dot`
dep-graph:
	cargo deps --optional-deps --filter wasmer-wasi wasmer-wasi-tests wasmer-kernel-loader wasmer-dev-utils wasmer-llvm-backend wasmer-emscripten wasmer-emscripten-tests wasmer-runtime-core wasmer-runtime wasmer-middleware-common wasmer-middleware-common-tests wasmer-singlepass-backend wasmer-clif-backend wasmer --manifest-path Cargo.toml | dot -Tpng > wasmer_depgraph.png

docs:
	cargo doc --features=backend-singlepass,backend-llvm,wasi,managed
