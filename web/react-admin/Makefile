.PHONY: install dev build preview lint lint-fix format docker-build docker-run clean help

IMAGE_NAME ?= react-admin
IMAGE_TAG ?= latest

install:
	pnpm install

dev:
	pnpm dev

build:
	pnpm build

preview:
	pnpm preview

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
	rm -rf node_modules dist

help:
	@echo "Available targets:"
	@echo "  install      - Install dependencies"
	@echo "  dev          - Start development server"
	@echo "  build        - Build for production"
	@echo "  preview      - Preview production build"
	@echo "  lint         - Run ESLint"
	@echo "  lint-fix     - Run ESLint with auto-fix"
	@echo "  format       - Format code with Prettier"
	@echo "  docker-build - Build Docker image"
	@echo "  docker-run   - Run Docker container"
	@echo "  clean        - Remove node_modules and dist"