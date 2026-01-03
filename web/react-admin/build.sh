#!/bin/bash

# Exit immediately if a command exits with a non-zero status
set -e

IMAGE_NAME="llm-proxy-admin-ui"
UPLOAD=false
RUN=false
BUILD=false
ENV_FILE=""
ARGS_FILE=""
LOCAL_PORT=8080
PORT=8080
FORCE_BUILD=false

# Parse command line arguments
ENV_TYPE=""
ARGS_TYPE=""
while [[ "$#" -gt 0 ]]; do
    case "$1" in
    --upload) UPLOAD=true ;;
    --run) RUN=true ;;
    --build) BUILD=true ;;
    --force-build) FORCE_BUILD=true ;;
    --env-json=*)
        if [ -n "$ENV_TYPE" ] && [ "$ENV_TYPE" != "json" ]; then
            echo "Error: Cannot use both --env-json and --env-file. Choose one."
            exit 1
        fi
        ENV_FILE="${1#*=}"
        ENV_TYPE="json"
        ;;
    --env-file=*)
        if [ -n "$ENV_TYPE" ] && [ "$ENV_TYPE" != "env" ]; then
            echo "Error: Cannot use both --env-json and --env-file. Choose one."
            exit 1
        fi
        ENV_FILE="${1#*=}"
        ENV_TYPE="env"
        ;;
    --args-json=*)
        if [ -n "$ARGS_TYPE" ] && [ "$ARGS_TYPE" != "json" ]; then
            echo "Error: Cannot use both --args-json and --args-file. Choose one."
            exit 1
        fi
        ARGS_FILE="${1#*=}"
        ARGS_TYPE="json"
        ;;
    --args-file=*)
        if [ -n "$ARGS_TYPE" ] && [ "$ARGS_TYPE" != "env" ]; then
            echo "Error: Cannot use both --args-json and --args-file. Choose one."
            exit 1
        fi
        ARGS_FILE="${1#*=}"
        ARGS_TYPE="env"
        ;;
    *)
        echo "Unknown parameter: $1"
        echo "Usage: $0 [--build] [--run] [--upload] [--force-build] [--env-file=FILE] [--env-json=FILE] [--args-file=FILE] [--args-json=FILE]"
        exit 1
        ;;
    esac
    shift
done

# Function to check if image exists
image_exists() {
    docker images --format "table {{.Repository}}:{{.Tag}}" | grep -q "^$IMAGE_NAME:latest$"
}

# Function to build image
build_image() {
    echo "Building Docker image..."
    
    # Prepare build args
    BUILD_ARGS=""
    if [ -n "$ARGS_FILE" ] && [ -f "$ARGS_FILE" ]; then
        echo "Using build args file: $ARGS_FILE"
        
        if [ "$ARGS_TYPE" = "json" ]; then
            # Handle JSON format - use temporary file to avoid process substitution
            BUILD_ARGS=""
            TEMP_FILE=$(mktemp)
            jq -r 'to_entries | .[] | "\(.key)=\(.value|tostring)"' "$ARGS_FILE" > "$TEMP_FILE"
            while IFS="=" read -r key value; do
                if [ -n "$key" ] && [ -n "$value" ]; then
                    BUILD_ARGS="$BUILD_ARGS --build-arg $(printf '%q' "$key")=$(printf '%q' "$value")"
                fi
            done < "$TEMP_FILE"
            rm -f "$TEMP_FILE"
        else
            # Handle .env format
            while IFS= read -r line || [ -n "$line" ]; do
                # Skip empty lines and comments
                if [[ -n "$line" && ! "$line" =~ ^[[:space:]]*# ]]; then
                    # Remove leading/trailing whitespace
                    line=$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
                    if [[ -n "$line" ]]; then
                        BUILD_ARGS="$BUILD_ARGS --build-arg $line"
                    fi
                fi
            done < "$ARGS_FILE"
        fi
    fi
    
    # Build with or without build args
    if [ -n "$BUILD_ARGS" ]; then
        echo "docker build --platform linux/amd64 $BUILD_ARGS -t $IMAGE_NAME ."
        eval "docker build --platform linux/amd64 $BUILD_ARGS -t $IMAGE_NAME ."
    else
        docker build --platform linux/amd64 -t "$IMAGE_NAME" .
    fi
    
    echo "Image built successfully: $IMAGE_NAME:latest"
}

# Function to run container
run_container() {
    echo "Running Docker container..."
    
    # Stop existing container if running
    if docker ps -q -f name="$IMAGE_NAME" | grep -q .; then
        echo "Stopping existing container..."
        docker stop "$IMAGE_NAME" || true
    fi
    
    # Remove existing container if exists
    if docker ps -aq -f name="$IMAGE_NAME" | grep -q .; then
        echo "Removing existing container..."
        docker rm "$IMAGE_NAME" || true
    fi

    # If no env file specified, check for default files
    if [ -z "$ENV_FILE" ]; then
        if [ -f ".env" ]; then
            ENV_FILE=".env"
            ENV_TYPE="env"
        elif [ -f "env.json" ]; then
            ENV_FILE="env.json"
            ENV_TYPE="json"
        elif [ -f ".env.json" ]; then
            ENV_FILE=".env.json"
            ENV_TYPE="json"
        fi
    fi

    if [ -n "$ENV_FILE" ] && [ -f "$ENV_FILE" ]; then
        echo "Using environment file: $ENV_FILE"
        
        # Handle based on ENV_TYPE
        if [ "$ENV_TYPE" = "json" ]; then
            # Handle JSON format
            ENV_ARGS=$(jq -r 'to_entries | map("-e '\''\(.key)=\(.value|tostring)'\''") | join(" ")' "$ENV_FILE")
        else
            # Handle .env format
            ENV_ARGS=""
            while IFS= read -r line || [ -n "$line" ]; do
                # Skip empty lines and comments
                if [[ -n "$line" && ! "$line" =~ ^[[:space:]]*# ]]; then
                    # Remove leading/trailing whitespace
                    line=$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
                    if [[ -n "$line" ]]; then
                        ENV_ARGS="$ENV_ARGS -e '$line'"
                    fi
                fi
            done < "$ENV_FILE"
        fi

        # Print the command
        echo "docker run --rm --name $IMAGE_NAME $ENV_ARGS -p $LOCAL_PORT:$PORT $IMAGE_NAME"

        # Run the container with environment variables
        eval "docker run --rm --name $IMAGE_NAME $ENV_ARGS -p $LOCAL_PORT:$PORT $IMAGE_NAME"
    else
        if [ -n "$ENV_FILE" ]; then
            echo "Warning: Specified environment file '$ENV_FILE' not found."
        else
            echo "Warning: No .env, env.json or .env.json file found."
        fi
        echo "Running without environment variables."
        docker run --rm --name $IMAGE_NAME -p $LOCAL_PORT:$PORT $IMAGE_NAME
    fi
}

# Function to upload image
upload_image() {
    # Create a tag based on current date and time (month, day, hour, minute)
    NEW_TAG=$(date +"%m%d%H%M")
    echo "Using tag: $NEW_TAG"
    
    # Tag the built images with the Tesla registry path
    echo "Tagging image..."
    docker tag $IMAGE_NAME:latest "nyuwa-user-docker-local.arf.tesla.cn/nyuwa-ns-voc/$IMAGE_NAME:$NEW_TAG"

    # Push the images to the registry
    echo "Pushing image to registry..."
    docker push "nyuwa-user-docker-local.arf.tesla.cn/nyuwa-ns-voc/$IMAGE_NAME:$NEW_TAG"

    echo "Complete! Image pushed with tag: $NEW_TAG"
    echo "Image name: nyuwa-user-docker-local.arf.tesla.cn/nyuwa-ns-voc/$IMAGE_NAME:$NEW_TAG"
}

# Main logic
echo "=== Docker Build Script ==="

# Build phase
if [ "$BUILD" = true ] || [ "$FORCE_BUILD" = true ]; then
    build_image
elif [ "$RUN" = true ] || [ "$UPLOAD" = true ]; then
    # Check if image exists when we need to run or upload
    if ! image_exists; then
        echo "Image $IMAGE_NAME:latest not found. Building..."
        build_image
    else
        echo "Using existing image: $IMAGE_NAME:latest"
    fi
fi

# Run phase
if [ "$RUN" = true ]; then
    run_container
fi

# Upload phase
if [ "$UPLOAD" = true ]; then
    if ! image_exists; then
        echo "Error: No image found to upload. Build first."
        exit 1
    fi
    upload_image
fi

# If no action specified, show usage
if [ "$BUILD" = false ] && [ "$RUN" = false ] && [ "$UPLOAD" = false ]; then
    echo "No action specified. Available options:"
    echo "  --build                Build Docker image"
    echo "  --run                  Run Docker container (builds if needed)"
    echo "  --upload               Upload to registry (builds if needed)"
    echo "  --force-build          Force rebuild even if image exists"
    echo "  --env-file=FILE        Use .env file for environment variables"
    echo "  --env-json=FILE        Use JSON file for environment variables"
    echo "  --args-file=FILE       Use .env file for build arguments"
    echo "  --args-json=FILE       Use JSON file for build arguments"
    echo ""
    echo "Examples:"
    echo "  $0 --build"
    echo "  $0 --build --args-file=build.args"
    echo "  $0 --build --args-json=build.json"
    echo "  $0 --run --env-file=.env"
    echo "  $0 --run --env-json=.env.json"
    echo "  $0 --upload"
    echo "  $0 --run --env-file=.env && $0 --upload"
fi