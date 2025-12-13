.PHONY: help build run upload force-build clean check-image

IMAGE_NAME := llm-proxy
REGISTRY := nyuwa-user-docker-local.arf.tesla.cn/nyuwa-ns-voc
PLATFORM := linux/amd64
LOCAL_PORT := 18000
PORT := 18000

# Default env/args files
ENV_FILE ?=
ARGS_FILE ?=
ENV_TYPE ?=
ARGS_TYPE ?=

# Generate tag based on current time
TAG := $(shell date +"%m%d%H%M")

help:
	@echo "=== Docker Build Makefile ==="
	@echo ""
	@echo "Available targets:"
	@echo "  make build              Build Docker image"
	@echo "  make run                Run Docker container (builds if needed)"
	@echo "  make upload             Upload to registry (builds if needed)"
	@echo "  make force-build        Force rebuild even if image exists"
	@echo "  make clean              Stop and remove container"
	@echo "  make check-image        Check if image exists"
	@echo ""
	@echo "Environment variables:"
	@echo "  ENV_FILE=FILE           Use .env file for environment variables"
	@echo "  ENV_JSON=FILE           Use JSON file for environment variables"
	@echo "  ARGS_FILE=FILE          Use .env file for build arguments"
	@echo "  ARGS_JSON=FILE          Use JSON file for build arguments"
	@echo ""
	@echo "Examples:"
	@echo "  make build"
	@echo "  make build ARGS_FILE=build.args"
	@echo "  make build ARGS_JSON=build.json"
	@echo "  make run ENV_FILE=.env"
	@echo "  make run ENV_JSON=.env.json"
	@echo "  make upload"
	@echo "  make run ENV_FILE=.env && make upload"

check-image:
	@if docker images --format "table {{.Repository}}:{{.Tag}}" | grep -q "^$(IMAGE_NAME):latest$$"; then \
		echo "Image $(IMAGE_NAME):latest exists"; \
		exit 0; \
	else \
		echo "Image $(IMAGE_NAME):latest not found"; \
		exit 1; \
	fi

build-docker-image:
	@echo "Building Docker image..."
	@BUILD_ARGS=""; \
	if [ -n "$(ARGS_JSON)" ] && [ -f "$(ARGS_JSON)" ]; then \
		echo "Using build args JSON file: $(ARGS_JSON)"; \
		TEMP_FILE=$$(mktemp); \
		jq -r 'to_entries | .[] | "\(.key)=\(.value|tostring)"' "$(ARGS_JSON)" > "$$TEMP_FILE"; \
		while IFS="=" read -r key value; do \
			if [ -n "$$key" ] && [ -n "$$value" ]; then \
				BUILD_ARGS="$$BUILD_ARGS --build-arg $$(printf '%q' "$$key")=$$(printf '%q' "$$value")"; \
			fi; \
		done < "$$TEMP_FILE"; \
		rm -f "$$TEMP_FILE"; \
	elif [ -n "$(ARGS_FILE)" ] && [ -f "$(ARGS_FILE)" ]; then \
		echo "Using build args file: $(ARGS_FILE)"; \
		while IFS= read -r line || [ -n "$$line" ]; do \
			if [[ -n "$$line" && ! "$$line" =~ ^[[:space:]]*\# ]]; then \
				line=$$(echo "$$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$$//'); \
				if [[ -n "$$line" ]]; then \
					BUILD_ARGS="$$BUILD_ARGS --build-arg $$line"; \
				fi; \
			fi; \
		done < "$(ARGS_FILE)"; \
	fi; \
	if [ -n "$$BUILD_ARGS" ]; then \
		echo "docker build --platform $(PLATFORM) $$BUILD_ARGS -t $(IMAGE_NAME) ."; \
		eval "docker build --platform $(PLATFORM) $$BUILD_ARGS -t $(IMAGE_NAME) ."; \
	else \
		docker build --platform $(PLATFORM) -t $(IMAGE_NAME) .; \
	fi
	@echo "Image built successfully: $(IMAGE_NAME):latest"

force-build: build

run:
	@echo "Running application locally..."
	@if [ -n "$(ENV_FILE)" ] && [ -f "$(ENV_FILE)" ]; then \
		echo "Using environment file: $(ENV_FILE)"; \
		export $$(grep -v '^#' $(ENV_FILE) | xargs); \
	elif [ -f ".env" ]; then \
		echo "Using default .env file"; \
		export $$(grep -v '^#' .env | xargs); \
	else \
		echo "No environment file found. Running without environment variables."; \
	fi; \
	python3 proxy.py

upload-docker-image:
	@if ! $(MAKE) -s check-image 2>/dev/null; then \
		echo "Image $(IMAGE_NAME):latest not found. Building..."; \
		$(MAKE) build; \
	fi
	@echo "Using tag: $(TAG)"
	@echo "Tagging image..."
	@docker tag $(IMAGE_NAME):latest "$(REGISTRY)/$(IMAGE_NAME):$(TAG)"
	@echo "Pushing image to registry..."
	@docker push "$(REGISTRY)/$(IMAGE_NAME):$(TAG)"
	@echo "Complete! Image pushed with tag: $(TAG)"
	@echo "Image name: $(REGISTRY)/$(IMAGE_NAME):$(TAG)"

clean:
	@echo "Stopping and removing container..."
	@docker stop $(IMAGE_NAME) 2>/dev/null || true
	@docker rm $(IMAGE_NAME) 2>/dev/null || true
	@echo "Container cleaned up"