# OTel CLI Lifecycle Logging Design

## Goal

Make OpenTelemetry logs useful for both the MCP server and one-shot CLI commands without relying on visible stderr. Every command must emit a bounded lifecycle, and its final events must be flushed before the process exits.

## Configuration

- Remove `--otel-log-level` and `OBSIDIAN_VAULT_MCP_OTEL_LOG_LEVEL`.
- Use `--log-level` as the single filter for stderr formatting, OpenTelemetry traces, and OpenTelemetry logs.
- Change the default `--log-level` from `warn` to `debug`.
- Keep `--otel-endpoint` and `--otel-service-name` unchanged.
- When no OTLP endpoint is configured, the same lifecycle events remain available through stderr according to `--log-level`.

## Lifecycle Events

After tracing and optional OTLP providers are initialized, emit:

- `telemetry.initialized` at `debug`, with an `otel.enabled` boolean.

Wrap the selected command in a single lifecycle and emit:

- `cli.command.start` at `debug`, with `command`.
- `cli.command.ok` at `debug`, with `command` and `duration_ms`.
- `cli.command.error` at `error`, with `command`, `duration_ms`, and `error`.

The `serve` command uses the same lifecycle. Its duration covers the server lifetime, while individual MCP calls continue to emit the existing `mcp.tool` span and `tool.call.*` events.

Command names are stable, low-cardinality identifiers derived from the enum variant. Lifecycle events must not contain vault paths, note content, command arguments, search text, regular expressions, or OTLP endpoint values.

## Execution and Shutdown Flow

1. Parse CLI arguments and determine the stable command name.
2. Initialize stderr tracing and, when configured, OTLP trace and log providers.
3. Emit `telemetry.initialized` and `cli.command.start`.
4. Execute the selected command and retain its `anyhow::Result`.
5. Emit `cli.command.ok` or `cli.command.error` before shutting down telemetry.
6. Force-flush and shut down both providers before returning the command result from `main`.

The existing ownership guard remains responsible for exactly-once provider shutdown. Explicit shutdown takes the providers out of the guard, so its `Drop` implementation is harmless.

Exporter flush or shutdown failures are written to stderr because the telemetry transport itself cannot be trusted at that point. They do not change a successful command into a failed command. The command's own success or failure remains the process result.

## Testing

Integration tests run the real binary against a temporary local OTLP HTTP receiver.

- A successful short CLI command must POST logs to `/v1/logs` before the child process exits. The payload must contain the initialization, start, and success event names.
- A failing CLI command must POST logs before exit, including the error event name.
- The CLI help output must no longer expose `--otel-log-level` and must show `debug` as the default for `--log-level`.
- Existing CLI and MCP integration tests must continue to pass.

The receiver only implements the minimal OTLP HTTP response needed by the SDK. Tests inspect the real exported protobuf payload rather than substituting a mocked logger provider, so they cover batching, force-flush, endpoint path construction, and process-exit ordering together.

## Documentation

Update the English and Chinese observability sections to describe the unified log level, the new default, lifecycle event names, and the guarantee that CLI completion and MCP shutdown flush pending telemetry.

## Non-goals

- Adding application metrics.
- Changing the remote Collector, Loki, Tempo, or Grafana configuration.
- Logging command arguments or vault data.
- Making telemetry delivery failures change CLI business-command exit status.
