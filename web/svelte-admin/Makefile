.PHONY: install dev build preview prepare check check-watch lint lint-fix format docker-build docker-run clean help

IMAGE_NAME ?= svelte-admin
IMAGE_TAG ?= latest

install:
	pnpm install

dev:
	pnpm dev

build:
	pnpm build

preview:
	pnpm preview

prepare:
	pnpm prepare

check:
	pnpm check

check-watch:
	pnpm check:watch

lint:
	pnpm lint

lint-fix:
	pnpm lint:fix

format:
	pnpm format

docker-build:
	docker build -t $(IMAGE_NAME):$(IMAGE_TAG) .

docker-run:
	docker run -p 8080:80 $(IMAGE_NAME):$(IMAGE_TAG)

clean:
	rm -rf node_modules build .svelte-kit

help:
	@echo "Available targets:"
	@echo "  install      - Install dependencies"
	@echo "  dev          - Start development server"
	@echo "  build        - Build for production"
	@echo "  preview      - Preview production build"
	@echo "  prepare      - Run svelte-kit sync"
	@echo "  check        - Run svelte-check"
	@echo "  check-watch  - Run svelte-check in watch mode"
	@echo "  lint         - Run ESLint"
	@echo "  lint-fix     - Run ESLint with auto-fix"
	@echo "  format       - Format code with Prettier"
	@echo "  docker-build - Build Docker image"
	@echo "  docker-run   - Run Docker container"
	@echo "  clean        - Remove node_modules, build and .svelte-kit"