fmt:
	cargo fmt --all

lint: fmt
	cargo clippy --fix --allow-dirty --allow-staged

build:
	docker build . -t soulgarden/logalert:0.0.8 --platform linux/amd64
	docker push soulgarden/logalert:0.0.8

create_namespace:
	kubectl create -f ./helm/namespace-logging.json

helm_install:
	helm install -n=logging logalert helm/logalert --wait

helm_upgrade:
	helm upgrade -n=logging logalert helm/logalert --wait

helm_delete:
	helm uninstall -n=logging logalert
