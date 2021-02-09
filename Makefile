SOURCES := $(shell find -name "*.rs" -print)

mapping-tools-web/static/wasm.js: $(SOURCES)
	wasm-pack build --target web --out-name wasm --out-dir static mapping-tools-web

