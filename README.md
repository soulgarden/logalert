# logalert

![Tests and linters](https://github.com/soulgarden/logalert/actions/workflows/main.yml/badge.svg)

Logalert collects events from elasticsearch/zincsearch and sends them to slack. The main goal is low memory and cpu consumption.

Supports ES 7.x, k8s 1.14+

### Install with helm
    make create_namespace

    make helm_install
