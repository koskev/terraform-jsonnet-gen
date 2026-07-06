.PHONY: test
test:
	mkdir -p build
	cp -r test/ build/
	tofu -chdir=build/test init
	cargo run -- --tf-dir build/test -o build/out
	find ./build -iname "*sonnet" | xargs -i jsonnet {}

.PHONY: clean
clean:
	rm -rf build
