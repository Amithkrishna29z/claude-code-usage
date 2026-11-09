# Usage data and refresh pipeline

## Sources

The monitor combines two complementary sources.

| Source | Inputs | Supplies | Failure behavior |
| --- | --- | --- | --- |
| Official | `claude_dir/.credentials.json` plus `GET https://api.anthropic.com/api/oauth/usage` | Session and weekly fractions/reset times | Returns `None`; use local data or current cache. |
| Local fallback | `claude_dir/projects/**/*.jsonl` | Session estimate, token totals, window/reset, latest activity | Produces a meaningful non-`Ok` state. |

The default Claude root is `~/.claude`; `claude_dir` overrides it. Local data is read
read-only. The OAuth token is sent only to `api.anthropic.com`.

The endpoint is internal and undocumented. Requests use `Authorization: Bearer
<token>` and `anthropic-beta: oauth-2025-04-20`. Only `five_hour` and `seven_day`
are read, defensively. Do not add an endpoint dependency without tests for missing,
malformed, and renamed data.

## Local log pipeline

1. `reader::read_events` recursively discovers `*.jsonl` under `projects/`.
2. It reports `logs_found` separately from parsed events: no files differs from files
   with no usable events.
3. Files untouched since the cutoff are not opened. The tree is still walked, and a
   file with unreadable metadata is read rather than silently dropped.
4. Files are streamed line-by-line. Invalid UTF-8 and unreadable files are skipped;
   warnings do not abort the remaining scan.
5. `parser::parse_line` fast-rejects lines without literal `"usage"`, then requires
   JSON `message.usage` and a top-level RFC 3339 timestamp.
6. Missing/non-numeric token fields are zero. The four fields are `input_tokens`,
   `output_tokens`, `cache_creation_input_tokens`, and `cache_read_input_tokens`.
7. Equal non-empty `message.id` values are counted once, including resumed sessions.
   Events without IDs are not de-duplicated.

`UsageEvent::total_tokens()` is the single canonical token-total definition. Changing
it changes every local estimate and requires tests and documentation updates.

## Local session-block algorithm

`calculator::compute` sorts events and models the active session using
`window_hours` (default 5.0; non-positive settings are treated as 5.0).

```text
Open a block at the first event.
For each later event, open a new block if either:
  event - block_start > window, OR
  event - previous_event > window.
Only the final block can be active now.
```

If `now >= final_block_start + window`, the snapshot is `NoActiveSession`, with zero
tokens and a 0% session window but no reset time. Otherwise utilization is
`block_tokens / token_limit`; a non-positive limit yields zero instead of dividing by
zero. Weekly usage is always `None` for local data.

The monitor reads only from `now - window_span(window_hours)` forward, because an
event before the sole potentially active block cannot affect the result.

## State meanings

| State | Meaning | Rendered signal |
| --- | --- | --- |
| `Ok` | Active local block or usable official session. | Live data; local source gets `est.`. |
| `NoActiveSession` | Logs exist but the final block elapsed. | 0% and `no session`. |
| `NoData` | Logs exist but yielded no usable usage event in range. | Em dash and `no data`. |
| `NoLogsFound` | No JSONL files beneath projects. | Em dash and `no logs`. |

For `Ok`, local activity older than ten minutes adds `stale`. That is activity
freshness, not endpoint cache freshness.

## Official fetch and cache policy

The worker reads local logs on every compute cycle, then overlays official session and
optional weekly values only if the official response includes a session window.

| Rule | Value | Reason |
| --- | --- | --- |
| Worker poll | 15 seconds | Re-evaluate timing and detect logs created after startup. |
| File-event debounce | 1.5 seconds | Coalesce write bursts. |
| Minimum endpoint fetch interval | 60 seconds | Avoid hard rate limiting. |
| Default idle refresh | 300 seconds | Do not re-fetch unchanged usage. |
| Last-good cache TTL after failure | 900 seconds | One failure should not immediately show estimate. |
| HTTP timeout | 10 seconds | Bound worker delay. |
| Default OAuth/429 backoff | 300 seconds | Prevent retry storms. |

The worker may fetch at the 60-second floor after **Refresh now**, a watcher refresh,
or newer local activity. Otherwise it waits for `max(refresh_seconds, 60)`. A 429
honors a positive numeric `Retry-After`; other rejected/status responses use the
five-minute backoff.

On fetch failure, the last successful official data remains displayable until its age
reaches 15 minutes. After that, cache is discarded and local data is shown. Missing or
invalid credentials, expired/rejected tokens, network failure, HTTP failures, and an
unknown response shape are all non-fatal.

## Watcher lifecycle limitation

The watcher starts only if `{claude_dir}/projects` exists, recursively watching only
`.jsonl` paths. The 15-second poll still discovers a directory created later, but
this monitor instance does not subsequently attach a watcher to it.

`Reconfigure` changes the worker config and clears official cache, but it does not
rebuild the watcher. Therefore a reloaded `claude_dir` affects reads/fetches but the
watcher remains on its startup directory until restart. Preserve this behavior unless
the request explicitly includes watcher lifecycle work and tests.

## Safe changes

- Keep parser changes defensive: one bad line must not lose a whole file or crash.
- Add fixed-timestamp tests for token fields, block rules, cache timing, or endpoint
  fields.
- Do not lower the 60-second fetch floor without upstream rate-limit evidence.
- Never turn an unrecognized official response into a misleading 0% value.
