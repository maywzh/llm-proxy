# Use Python 3.12 slim image
FROM --platform=linux/amd64 cndevops-base-docker-local.arf.tesla.cn/base/python:3.12-alpine



# Set working directory
WORKDIR /app

# Install uv
RUN pip install uv

# Copy project files
COPY pyproject.toml ./
COPY uv.lock ./
COPY proxy.py ./

# Copy default config and certificate if they exist
COPY . /tmp/build/
RUN if [ -f /tmp/build/config.yaml ]; then cp /tmp/build/config.yaml ./config.yaml; fi && \
    if [ -f /tmp/build/cacerts.pem ]; then cp /tmp/build/cacerts.pem ./cacerts.pem; fi && \
    rm -rf /tmp/build

# Install dependencies using uv
RUN uv sync --frozen

# Expose port (default 8000, can be overridden by config)
EXPOSE 8000

# Set default config path (can be overridden by environment variable)
ENV CONFIG_PATH=/app/config.yaml

# Run the application
# Can be overridden in docker-compose or docker run with custom config path
CMD ["sh", "-c", "uv run proxy.py --config=${CONFIG_PATH}"]
