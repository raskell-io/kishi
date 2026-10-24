PORT ?= 8647
WASM := target/wasm32-unknown-unknown/release/kishi_engine.wasm
WASM_SHOGI := target/wasm32-unknown-unknown/release/kishi_shogi.wasm
WASM_GO := target/wasm32-unknown-unknown/release/kishi_go.wasm
WASM_MT := target/mt/wasm32-unknown-unknown/release/kishi_engine.wasm
# Shared memory and the exports helper workers need to set up their own stack and TLS.
MT_FLAGS := -C target-feature=+atomics,+bulk-memory,+mutable-globals,+simd128,+nontrapping-fptoint \
	-C link-arg=--shared-memory -C link-arg=--import-memory -C link-arg=--max-memory=268435456 \
	-C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size \
	-C link-arg=--export=__tls_align -C link-arg=--export=__stack_pointer
WASM_OPT := --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int
DIST := dist
THREADS ?= $(shell sysctl -n hw.ncpu 2>/dev/null || nproc)

.PHONY: build build-threads serve site test tools bench clean

# The engines, compiled for the web page: chess (with nets/kishi.nnue embedded),
# single-threaded and, with a nightly toolchain, threaded; shogi; and Go.
build:
	cargo build --release --target wasm32-unknown-unknown -p kishi-engine --features web
	@if command -v wasm-opt >/dev/null; then wasm-opt -O3 $(WASM_OPT) $(WASM) -o web/kishi.wasm; \
	else cp $(WASM) web/kishi.wasm; fi
	@ls -l web/kishi.wasm | awk '{print "web/kishi.wasm: " $$5 " bytes"}'
	cargo build --release --target wasm32-unknown-unknown -p kishi-shogi --features web
	@if command -v wasm-opt >/dev/null; then wasm-opt -O3 $(WASM_OPT) $(WASM_SHOGI) -o web/shogi.wasm; \
	else cp $(WASM_SHOGI) web/shogi.wasm; fi
	@ls -l web/shogi.wasm | awk '{print "web/shogi.wasm: " $$5 " bytes"}'
	cargo build --release --target wasm32-unknown-unknown -p kishi-go --features web
	@if command -v wasm-opt >/dev/null; then wasm-opt -O3 $(WASM_OPT) $(WASM_GO) -o web/go.wasm; \
	else cp $(WASM_GO) web/go.wasm; fi
	@ls -l web/go.wasm | awk '{print "web/go.wasm: " $$5 " bytes"}'
	@if rustup toolchain list 2>/dev/null | grep -q nightly; then $(MAKE) --no-print-directory build-threads; \
	else echo "no nightly toolchain: skipping web/kishi-mt.wasm"; rm -f web/kishi-mt.wasm; fi

build-threads:
	RUSTFLAGS="$(MT_FLAGS)" cargo +nightly build -Z build-std=std,panic_abort --release \
		--target wasm32-unknown-unknown -p kishi-engine --features threads --target-dir target/mt
	@if command -v wasm-opt >/dev/null; then wasm-opt -O3 $(WASM_OPT) --enable-threads --enable-mutable-globals $(WASM_MT) -o web/kishi-mt.wasm; \
	else cp $(WASM_MT) web/kishi-mt.wasm; fi
	@ls -l web/kishi-mt.wasm | awk '{print "web/kishi-mt.wasm: " $$5 " bytes"}'

# Serves with the cross-origin isolation headers the threaded engine needs.
serve: build
	@echo "http://localhost:$(PORT)/"
	python3 scripts/serve.py $(PORT) web

# The website is the web page as it stands, in dist/.
site: build
	rm -rf $(DIST)
	cp -R web $(DIST)

test:
	cargo test --release

tools:
	cargo build --release -p kishi-tools

bench: tools
	./target/release/kishi bench

clean:
	cargo clean
	rm -rf web/kishi.wasm $(DIST)
