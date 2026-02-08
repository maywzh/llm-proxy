---
name: analyzing-error-logs
description: Use when user asks to check error logs, analyze API errors, investigate provider failures, or generate error distribution reports for the llm-proxy project
allowed-tools: Bash Read Write Edit
---

## Always run the query script with `--help` first to see available modes and options.

**Script path**: `.claude/skills/analyzing-error-logs/scripts/query_error_logs.sh`

# Analyzing Error Logs

Query and analyze the `error_logs` table in the llm-proxy PostgreSQL database to diagnose API proxy issues.

## Prerequisites

- `psql` CLI available
- `.env` file with `DB_URL` at `rust-server/.env` (auto-detected by script)

## Database Schema

**Table**: `error_logs` (PostgreSQL, indexed on `timestamp`, `error_category`, `provider_name`, `request_id`)

| Column | Type | Notes |
|--------|------|-------|
| `id` | BIGSERIAL | PK |
| `request_id` | VARCHAR(64) | Unique request ID |
| `timestamp` | TIMESTAMPTZ | Error time (indexed) |
| `error_category` | VARCHAR(32) | See categories below (indexed) |
| `error_code` | INTEGER | |
| `error_message` | TEXT | |
| `endpoint` | VARCHAR(255) | e.g. `/v1/chat/completions` |
| `client_protocol` | VARCHAR(32) | `openai`, `anthropic`, etc. |
| `provider_name` | VARCHAR(255) | Provider identifier (indexed) |
| `provider_api_base` | VARCHAR(500) | |
| `provider_protocol` | VARCHAR(32) | |
| `mapped_model` | VARCHAR(255) | Model after mapping |
| `response_status_code` | INTEGER | HTTP status from provider |
| `response_body` | JSONB | Provider's error response |
| `total_duration_ms` | INTEGER | Request duration |
| `credential_name` | VARCHAR(255) | API key identifier |
| `client` | VARCHAR(255) | Calling application |
| `is_streaming` | BOOLEAN | |
| `request_headers` | JSONB | Sanitized (auth masked) |
| `request_body` | JSONB | Truncated (200 byte limit) |
| `provider_request_body` | JSONB | Request sent to provider |
| `provider_request_headers` | JSONB | Headers sent to provider |

## Error Categories

| Category | Meaning | Common Causes |
|----------|---------|---------------|
| `provider_4xx` | Provider returned 4xx | Rate limit (429), auth failure (401/403), bad request (400) |
| `provider_5xx` | Provider returned 5xx | Provider outage, overload |
| `timeout` | Request timed out | Slow TTFT, network congestion |
| `network_error` | Network transport failure | DNS failure, connection reset |
| `connect_error` | Connection establishment failed | Provider unreachable |
| `stream_error` | Streaming interrupted | Mid-stream disconnect |
| `internal_error` | Proxy internal error | Bug in proxy logic |

## Workflow

### Quick Check (single command)

```bash
SCRIPT=".claude/skills/analyzing-error-logs/scripts/query_error_logs.sh"
$SCRIPT overview --hours 1
$SCRIPT by-category --hours 24
$SCRIPT detail --provider "provider-name" --limit 10
```

### Full Investigation

1. **Overview**: `$SCRIPT overview --hours N`
2. **Identify pattern**: `by-category` or `by-provider` to find the dominant error source
3. **Drill down**: `cross` for provider x category matrix, then `detail --provider X` for samples
4. **Check trend**: `trend --hours 24` to see if worsening or recovering
5. **Root cause — response body**: `response-body --provider X` to read upstream error messages
6. **If response_body is NULL**: move to step 7 (this can indicate a code bug or network-level failure)
7. **Request size analysis**: `request-size --provider X` to check message/tool counts and payload sizes
8. **Protocol check**: Look at `client_protocol` vs `provider_protocol` in the output — mismatches (e.g. `anthropic` -> `openai`) indicate protocol conversion issues that can cause failures

### Deep-Dive: When response_body is NULL

When `response-body` returns all NULLs, the upstream never sent a parseable error body. Investigate via request characteristics instead:

1. **Check request sizes**: `$SCRIPT request-size --provider X --limit 30`
   - Large `body_bytes` (>100KB) may hit provider payload limits
   - High `msg_count` or `tool_count` may exceed context or tool limits
2. **Check protocol conversion**: Look at `client_protocol` vs `provider_protocol` columns
   - Mismatch like `anthropic` -> `openai` means the proxy is converting protocols, which can introduce serialization bugs
3. **Cross-reference with working requests**: Compare failing request sizes with typical successful ones
4. **Custom SQL for deeper analysis**:
   ```bash
   DB_URL=$(grep '^DB_URL=' rust-server/.env | head -1 | sed "s/^DB_URL=//;s/^'//;s/'$//")
   # Check distribution of request sizes for a specific provider
   psql "$DB_URL" -c "
     SELECT
       client_protocol, provider_protocol,
       ROUND(AVG(octet_length(provider_request_body::text))) AS avg_body_bytes,
       ROUND(AVG(jsonb_array_length(provider_request_body->'messages'))) AS avg_msgs,
       ROUND(AVG(COALESCE(jsonb_array_length(provider_request_body->'tools'), 0))) AS avg_tools,
       COUNT(*) AS cnt
     FROM error_logs
     WHERE timestamp >= NOW() - INTERVAL '24 hours'
       AND provider_name = 'PROVIDER_HERE'
     GROUP BY client_protocol, provider_protocol
     ORDER BY cnt DESC;
   "
   ```

### Full Report

```bash
$SCRIPT full-report --hours 24
```

### Custom SQL (when script doesn't cover the case)

```bash
DB_URL=$(grep '^DB_URL=' rust-server/.env | head -1 | sed "s/^DB_URL=//;s/^'//;s/'$//")
psql "$DB_URL" -c "SELECT ... FROM error_logs WHERE ..."
```

## Diagnosis Quick Reference

| Symptom | Query Mode | What to Check |
|---------|-----------|---------------|
| "Errors spiking" | `trend --hours 6` | Sudden increase = provider outage; gradual = quota exhaustion |
| "Provider failing" | `response-body --provider X` | Check response preview for upstream message |
| "Provider failing, NULL response" | `request-size --provider X` | Check payload sizes and protocol conversion |
| "Rate limited" | `cross` then filter `provider_4xx` | 429 status -> credential rotation or adaptive routing |
| "Auth errors" | `by-credential --provider X` | 401/403 -> specific credential expired |
| "Slow responses" | `by-model` | High `avg_ms` -> model/provider latency issue |
| "Specific client issues" | `by-client` | One client dominates -> likely bad request patterns |
| "Protocol mismatch" | `request-size --provider X` | `client_protocol` != `provider_protocol` -> conversion issue |
| "Large payload errors" | `request-size --limit 50` | Sort by `body_bytes` to find oversized requests |

## Report Output

Write analysis results to `.analyse/{yyyyMMddhhmm}_error_log_analysis.md` with structure:

1. **TL;DR** -- One-line summary
2. **Overview** -- Total errors, time range, scope
3. **Category Distribution** -- Table with counts and percentages
4. **Provider Analysis** -- Per-provider breakdown
5. **Time Trend** -- Spike detection and recovery patterns
6. **Root Cause** -- Based on `response_body` samples; if NULL, based on request size / protocol analysis
7. **Recommendations** -- Actionable next steps

## Common Mistakes

| Mistake | Fix |
|---------|-----|
| Not specifying `--hours` | Default is 1 hour; use `--hours 24` for broader view |
| Ignoring `response_body` | This JSONB field contains the upstream provider's actual error message -- always check it first |
| Stopping when response_body is NULL | Use `request-size` mode to analyze request characteristics instead |
| Ignoring protocol mismatch | Check `client_protocol` vs `provider_protocol` -- conversion bugs are a common root cause |
| Missing time zone | Script outputs CST (Asia/Shanghai); raw SQL should use `AT TIME ZONE 'Asia/Shanghai'` |
| Querying without index | Always filter by `timestamp`, `error_category`, or `provider_name` for performance |
| Using wrong script path | Script is at `.claude/skills/analyzing-error-logs/scripts/query_error_logs.sh`, not `scripts/query_error_logs.sh` |
