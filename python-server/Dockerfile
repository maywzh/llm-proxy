# Use Python 3.12 slim image
FROM --platform=linux/amd64 cndevops-base-docker-local.arf.tesla.cn/base/python:3.12-alpine

# Set working directory
WORKDIR /app

# Install uv
RUN pip install uv

# Copy project files
COPY pyproject.toml ./
COPY uv.lock ./
COPY main.py ./
COPY app/ ./app/

# Copy certificate if it exists
COPY . /tmp/build/
RUN if [ -f /tmp/build/cacerts.pem ]; then cp /tmp/build/cacerts.pem ./cacerts.pem; fi && \
    rm -rf /tmp/build

# Install dependencies using uv
RUN uv sync --frozen

# Expose port (default 18000)
EXPOSE 18000

# Run the application
# Configuration is loaded from database via DB_URL environment variable
CMD ["uv", "run", "python", "main.py"]
