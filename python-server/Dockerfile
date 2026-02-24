FROM python:3.12-alpine

WORKDIR /app

RUN pip install uv

COPY pyproject.toml uv.lock ./
RUN uv sync --frozen

COPY main.py ./
COPY app/ ./app/

EXPOSE 18000

CMD ["uv", "run", "python", "main.py"]
