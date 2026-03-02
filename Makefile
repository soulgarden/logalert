SHELL := /bin/bash

VERSION_FILE := VERSION
VERSION := $(shell cat $(VERSION_FILE))
IMAGE_REPO ?= soulgarden
IMAGE_NAME ?= logalert
IMAGE := $(IMAGE_REPO)/$(IMAGE_NAME)
PLATFORM ?= linux/amd64
CHART_PATH := helm/logalert
CARGO_MANIFEST := Cargo.toml
CARGO_LOCK := Cargo.lock
CHART_FILE := $(CHART_PATH)/Chart.yaml
VALUES_FILE := $(CHART_PATH)/values.yaml

.PHONY: fmt fmt-check lint lint_fix test check ci \
	build docker-build \
	create_namespace helm_install helm_upgrade helm_delete \
	get-version increment-version

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets -- -D warnings

lint_fix:
	cargo clippy --all-targets --fix --allow-dirty --allow-staged -- -D warnings

test:
	cargo test -- --test-threads=1

check:
	cargo check

ci: fmt-check lint test check
	@echo "CI pipeline completed successfully!"

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
	sed -E 's/^appVersion: ".*"$$/appVersion: "'"$$new_version"'"/' $(CHART_FILE) > $(CHART_FILE).tmp && mv $(CHART_FILE).tmp $(CHART_FILE); \
	sed -E 's/^([[:space:]]*tag: ).*/\1"'"$$new_version"'"/' $(VALUES_FILE) > $(VALUES_FILE).tmp && mv $(VALUES_FILE).tmp $(VALUES_FILE)

build:
	docker build . --platform $(PLATFORM) \
		-t $(IMAGE):$(VERSION) \
		-t $(IMAGE):latest
	docker push $(IMAGE):$(VERSION)
	docker push $(IMAGE):latest

docker-build: build

create_namespace:
	kubectl create -f ./helm/namespace-logging.json

helm_install:
	helm install -n=logging logalert $(CHART_PATH) --wait \
		--set image.tag=$(VERSION)

helm_upgrade:
	helm upgrade -n=logging logalert $(CHART_PATH) --wait \
		--set image.tag=$(VERSION)

helm_delete:
	helm uninstall -n=logging logalert
