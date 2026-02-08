#!/usr/bin/env bash
# Usage: query_error_logs.sh <mode> [options]
#
# Modes:
#   overview       [--hours N]           Overall error stats (default: 1 hour)
#   by-category    [--hours N]           Group by error_category
#   by-provider    [--hours N]           Group by provider_name
#   cross          [--hours N]           Provider x Category cross table
#   trend          [--hours N] [--bucket INTERVAL]  Time series (default bucket: 5 minutes)
#   top-errors     [--hours N] [--limit N]  Top error messages
#   detail         [--hours N] [--limit N] [--provider NAME] [--category NAME]
#   response-body  [--hours N] [--limit N] [--provider NAME]  Show response body previews
#   request-size   [--hours N] [--limit N] [--provider NAME]  Analyze request payload sizes
#   by-model       [--hours N]           Group by mapped_model
#   by-client      [--hours N]           Group by client
#   by-credential  [--hours N] [--provider NAME]  Group by credential_name
#   full-report    [--hours N]           Run all queries and output combined report
#
# Options:
#   --hours N       Time window in hours (default: 1)
#   --bucket INTERVAL  Time bucket for trend (default: '5 minutes')
#   --limit N       Row limit for detail/top-errors/response-body/request-size (default: 20)
#   --provider NAME Filter by provider_name
#   --category NAME Filter by error_category
#   --env FILE      Path to .env file (default: auto-detect)
#   --help          Show this help

set -euo pipefail

# --- Defaults ---
MODE=""
HOURS=1
BUCKET="5 minutes"
LIMIT=20
PROVIDER_FILTER=""
CATEGORY_FILTER=""
ENV_FILE=""

# --- Parse args ---
while [[ $# -gt 0 ]]; do
  case "$1" in
    overview|by-category|by-provider|cross|trend|top-errors|detail|response-body|request-size|by-model|by-client|by-credential|full-report)
      MODE="$1"; shift ;;
    --hours) HOURS="$2"; shift 2 ;;
    --bucket) BUCKET="$2"; shift 2 ;;
    --limit) LIMIT="$2"; shift 2 ;;
    --provider) PROVIDER_FILTER="$2"; shift 2 ;;
    --category) CATEGORY_FILTER="$2"; shift 2 ;;
    --env) ENV_FILE="$2"; shift 2 ;;
    --help|-h) head -24 "$0" | tail -23; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$MODE" ]]; then
  echo "Usage: $0 <mode> [options]  (use --help for details)" >&2
  exit 1
fi

# --- Locate .env and extract DB_URL ---
if [[ -z "$ENV_FILE" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
  PROJECT_ROOT="$SCRIPT_DIR/../../../.."
  for candidate in \
    "$PROJECT_ROOT/rust-server/.env" \
    "$(pwd)/rust-server/.env" \
    "$(pwd)/.env"; do
    if [[ -f "$candidate" ]]; then
      ENV_FILE="$candidate"
      break
    fi
  done
fi

if [[ -z "$ENV_FILE" || ! -f "$ENV_FILE" ]]; then
  echo "Error: Cannot find .env file. Use --env to specify." >&2
  exit 1
fi

DB_URL=$(grep '^DB_URL=' "$ENV_FILE" | head -1 | sed "s/^DB_URL=//; s/^['\"]//; s/['\"]$//")
if [[ -z "$DB_URL" ]]; then
  echo "Error: DB_URL not found in $ENV_FILE" >&2
  exit 1
fi

export PGPASSWORD=""  # clear any inherited password

# --- Helper: run SQL ---
run_sql() {
  psql "$DB_URL" -t -A -F $'\t' -c "$1" 2>/dev/null
}

run_sql_pretty() {
  psql "$DB_URL" -c "$1" 2>/dev/null
}

# --- Build WHERE clause ---
build_where() {
  local where="timestamp >= NOW() - INTERVAL '$HOURS hours'"
  [[ -n "$PROVIDER_FILTER" ]] && where="$where AND provider_name = '$PROVIDER_FILTER'"
  [[ -n "$CATEGORY_FILTER" ]] && where="$where AND error_category = '$CATEGORY_FILTER'"
  echo "$where"
}

WHERE=$(build_where)

# --- Queries ---
query_overview() {
  echo "=== Error Overview (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      COUNT(*) AS total_errors,
      MIN(timestamp AT TIME ZONE 'Asia/Shanghai')::text AS first_error_cst,
      MAX(timestamp AT TIME ZONE 'Asia/Shanghai')::text AS last_error_cst,
      COUNT(DISTINCT provider_name) AS providers,
      COUNT(DISTINCT error_category) AS categories,
      COUNT(DISTINCT mapped_model) AS models,
      COUNT(DISTINCT client) AS clients
    FROM error_logs
    WHERE $WHERE;
  "
}

query_by_category() {
  echo "=== Errors by Category (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      error_category,
      COUNT(*) AS cnt,
      ROUND(COUNT(*) * 100.0 / NULLIF(SUM(COUNT(*)) OVER(), 0), 1) AS pct,
      ROUND(AVG(total_duration_ms)) AS avg_ms,
      MAX(timestamp AT TIME ZONE 'Asia/Shanghai')::text AS last_seen_cst
    FROM error_logs
    WHERE $WHERE
    GROUP BY error_category
    ORDER BY cnt DESC;
  "
}

query_by_provider() {
  echo "=== Errors by Provider (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      provider_name,
      COUNT(*) AS cnt,
      ROUND(COUNT(*) * 100.0 / NULLIF(SUM(COUNT(*)) OVER(), 0), 1) AS pct,
      COUNT(DISTINCT error_category) AS cats,
      COUNT(DISTINCT mapped_model) AS models,
      ROUND(AVG(total_duration_ms)) AS avg_ms
    FROM error_logs
    WHERE $WHERE
    GROUP BY provider_name
    ORDER BY cnt DESC;
  "
}

query_cross() {
  echo "=== Provider x Category Cross Table (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      provider_name,
      error_category,
      COUNT(*) AS cnt,
      ROUND(COUNT(*) * 100.0 / NULLIF(SUM(COUNT(*)) OVER(PARTITION BY provider_name), 0), 1) AS pct_in_provider,
      ARRAY_AGG(DISTINCT response_status_code) FILTER (WHERE response_status_code IS NOT NULL) AS status_codes
    FROM error_logs
    WHERE $WHERE
    GROUP BY provider_name, error_category
    ORDER BY provider_name, cnt DESC;
  "
}

query_trend() {
  echo "=== Error Trend (last ${HOURS}h, bucket=${BUCKET}) ==="
  run_sql_pretty "
    SELECT
      (date_bin('${BUCKET}', timestamp, TIMESTAMPTZ '2000-01-01') AT TIME ZONE 'Asia/Shanghai')::text AS time_cst,
      error_category,
      COUNT(*) AS cnt
    FROM error_logs
    WHERE $WHERE
    GROUP BY time_cst, error_category
    ORDER BY time_cst DESC, cnt DESC;
  "
}

query_top_errors() {
  echo "=== Top Error Messages (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      provider_name,
      error_category,
      response_status_code AS status,
      LEFT(error_message, 150) AS message_preview,
      COUNT(*) AS cnt,
      MAX(timestamp AT TIME ZONE 'Asia/Shanghai')::text AS last_seen_cst
    FROM error_logs
    WHERE $WHERE
    GROUP BY provider_name, error_category, response_status_code, LEFT(error_message, 150)
    ORDER BY cnt DESC
    LIMIT $LIMIT;
  "
}

query_detail() {
  echo "=== Error Details (last ${HOURS}h, limit=${LIMIT}) ==="
  run_sql_pretty "
    SELECT
      id,
      (timestamp AT TIME ZONE 'Asia/Shanghai')::text AS time_cst,
      error_category,
      response_status_code AS status,
      provider_name,
      mapped_model,
      credential_name,
      client,
      is_streaming,
      total_duration_ms AS ms,
      LEFT(error_message, 120) AS message,
      LEFT(response_body::text, 200) AS response_preview
    FROM error_logs
    WHERE $WHERE
    ORDER BY timestamp DESC
    LIMIT $LIMIT;
  "
}

query_response_body() {
  echo "=== Response Body Preview (last ${HOURS}h, limit=${LIMIT}) ==="
  run_sql_pretty "
    SELECT
      id,
      (timestamp AT TIME ZONE 'Asia/Shanghai')::text AS time_cst,
      provider_name,
      response_status_code AS status,
      LEFT(response_body::text, 500) AS response_preview,
      LEFT(error_message, 100) AS message
    FROM error_logs
    WHERE $WHERE
    ORDER BY timestamp DESC
    LIMIT $LIMIT;
  "
}

query_request_size() {
  echo "=== Request Size Analysis (last ${HOURS}h, limit=${LIMIT}) ==="
  run_sql_pretty "
    SELECT
      id,
      (timestamp AT TIME ZONE 'Asia/Shanghai')::text AS time_cst,
      provider_name,
      credential_name,
      jsonb_array_length(provider_request_body->'messages') AS msg_count,
      jsonb_array_length(provider_request_body->'tools') AS tool_count,
      octet_length(provider_request_body::text) AS body_bytes,
      client_protocol,
      provider_protocol,
      total_duration_ms AS ms
    FROM error_logs
    WHERE $WHERE
    ORDER BY timestamp DESC
    LIMIT $LIMIT;
  "
}

query_by_model() {
  echo "=== Errors by Model (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      mapped_model,
      provider_name,
      error_category,
      COUNT(*) AS cnt,
      ROUND(AVG(total_duration_ms)) AS avg_ms
    FROM error_logs
    WHERE $WHERE
      AND mapped_model IS NOT NULL AND mapped_model != ''
    GROUP BY mapped_model, provider_name, error_category
    ORDER BY cnt DESC
    LIMIT $LIMIT;
  "
}

query_by_client() {
  echo "=== Errors by Client (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      client,
      COUNT(*) AS cnt,
      COUNT(DISTINCT error_category) AS cats,
      COUNT(DISTINCT provider_name) AS providers
    FROM error_logs
    WHERE $WHERE
    GROUP BY client
    ORDER BY cnt DESC;
  "
}

query_by_credential() {
  echo "=== Errors by Credential (last ${HOURS}h) ==="
  run_sql_pretty "
    SELECT
      credential_name,
      provider_name,
      error_category,
      COUNT(*) AS cnt,
      COUNT(DISTINCT mapped_model) AS models
    FROM error_logs
    WHERE $WHERE
    GROUP BY credential_name, provider_name, error_category
    ORDER BY cnt DESC;
  "
}

# --- Execute ---
case "$MODE" in
  overview)       query_overview ;;
  by-category)    query_by_category ;;
  by-provider)    query_by_provider ;;
  cross)          query_cross ;;
  trend)          query_trend ;;
  top-errors)     query_top_errors ;;
  detail)         query_detail ;;
  response-body)  query_response_body ;;
  request-size)   query_request_size ;;
  by-model)       query_by_model ;;
  by-client)      query_by_client ;;
  by-credential)  query_by_credential ;;
  full-report)
    query_overview
    echo ""
    query_by_category
    echo ""
    query_by_provider
    echo ""
    query_cross
    echo ""
    query_trend
    echo ""
    query_top_errors
    echo ""
    query_by_model
    echo ""
    query_by_client
    echo ""
    query_by_credential
    ;;
esac
