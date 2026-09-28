# Put flags in SHELL for compatibility with macOS GNU Make 3.81.
SHELL := /bin/bash -eu -o pipefail
.DEFAULT_GOAL := ci

VERSION_FILE := VERSION
VERSION := $(shell cat $(VERSION_FILE))
IMAGE_REPO ?= soulgarden
IMAGE_NAME ?= logalert
IMAGE := $(IMAGE_REPO)/$(IMAGE_NAME)
PLATFORM ?= linux/amd64
NAMESPACE ?= logging
RELEASE ?= logalert
CHART_PATH := helm/logalert
CARGO_MANIFEST := Cargo.toml
CARGO_LOCK := Cargo.lock
CHART_FILE := $(CHART_PATH)/Chart.yaml

.PHONY: fmt fmt-check lint lint_fix test check ci \
	build docker-build push docker-push \
	create_namespace helm_install helm_upgrade helm_delete \
	get-version increment-version

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --locked --all-targets --all-features -- -D warnings

lint_fix:
	cargo clippy --locked --all-targets --all-features --fix --allow-dirty --allow-staged -- -D warnings

test:
	cargo test --locked --all-targets --all-features -- --test-threads=1

check:
	cargo check --locked --all-targets --all-features

ci: fmt-check lint test check

get-version:
	@cat $(VERSION_FILE)

increment-version:
	@current_version="$$(cat $(VERSION_FILE))"; \
	major="$${current_version%%.*}"; \
	rest="$${current_version#*.}"; \
	minor="$${rest%%.*}"; \
	new_version="$$major.$$((minor + 1)).0"; \
	echo "Bump version: $$current_version -> $$new_version"; \
	printf "%s\n" "$$new_version" > $(VERSION_FILE); \
	sed -E '/^\[package\]$$/,/^\[/{s/^version = ".*"$$/version = "'"$$new_version"'"/;}' $(CARGO_MANIFEST) > $(CARGO_MANIFEST).tmp && mv $(CARGO_MANIFEST).tmp $(CARGO_MANIFEST); \
	sed -E '/^name = "logalert"$$/{n;s/^version = ".*"$$/version = "'"$$new_version"'"/;}' $(CARGO_LOCK) > $(CARGO_LOCK).tmp && mv $(CARGO_LOCK).tmp $(CARGO_LOCK); \
	sed -E 's/^version: .*/version: '"$$new_version"'/' $(CHART_FILE) > $(CHART_FILE).tmp && mv $(CHART_FILE).tmp $(CHART_FILE); \
	sed -E 's/^appVersion: ".*"$$/appVersion: "'"$$new_version"'"/' $(CHART_FILE) > $(CHART_FILE).tmp && mv $(CHART_FILE).tmp $(CHART_FILE)

build: docker-build

docker-build:
	docker buildx build --load --platform "$(PLATFORM)" \
		-t "$(IMAGE):$(VERSION)" \
		-t "$(IMAGE):latest" .

push: docker-push

docker-push: docker-build
	docker push "$(IMAGE):$(VERSION)"
	docker push "$(IMAGE):latest"

create_namespace:
	kubectl create -f ./helm/namespace-logging.json

helm_install:
	helm install --namespace "$(NAMESPACE)" "$(RELEASE)" "$(CHART_PATH)" --wait \
		--set-string image.tag="$(VERSION)"

helm_upgrade:
	helm upgrade --namespace "$(NAMESPACE)" "$(RELEASE)" "$(CHART_PATH)" --wait \
		--set-string image.tag="$(VERSION)"

helm_delete:
	helm uninstall --namespace "$(NAMESPACE)" "$(RELEASE)"
