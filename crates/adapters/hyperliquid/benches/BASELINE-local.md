# Hyperliquid Adapter Benchmarks — Local Baseline

This is a local Apple Silicon baseline. It does **not** replace
[`BENCHMARKS.md`](BENCHMARKS.md), which records the upstream Threadripper
figures.

**Every number in this file is CPU work. None of it is production latency.**
Nothing here measures async scheduling, the PostRouter, channel hops, handler
serialization, or the socket write.

`ExecutionClient::submit_order` is excluded: it returns immediately after
`task_spawner.spawn` (execution.rs:869) with no completion signal.
Benchmarking it would spawn one unbounded task per iteration. That path is
deferred to a later mock-WebSocket benchmark.

`submit_path/convert_order` and `submit_path/sign_request` must not be summed.
They measure different functions on different inputs.

## Measurement record

| Field               | Value                                                                 |
| ------------------- | --------------------------------------------------------------------- |
| Hardware            | Apple M4 Pro (14 cores)                                               |
| OS                  | macOS 26.5.2                                                          |
| rustc               | 1.98.1 (48a229cea 2026-09-01)                                         |
| Profile             | `bench-lto` (release + `lto = "fat"` + `codegen-units = 1`, `debug = full`) |
| Commit              | `9d6cad6` plus uncommitted harness files on `bench/hyperliquid-baseline` |
| Date                | 2026-09-15                                                            |
| Sample size         | Criterion default (100)                                               |
| Aggregate           | Single full run (not median-of-5)                                     |
| Cargo jobs          | `-j 1`                                                                |

`cpupower frequency-set` and `setarch -R` from [`BENCHMARKS.md`](BENCHMARKS.md)
are Linux-only and unavailable on macOS. This capture used AC power. Only one
full `bench-lto` session completed; four more would be needed for median-of-5.

Criterion times below are the middle value Criterion printed (typical estimate
with 100 samples). `alloc_probe` stdout scrolled out of the terminal; numbers
are from re-running the already-built `bench-lto` binary.

## How to reproduce

```bash
cargo bench -p nautilus-hyperliquid --profile bench-lto \
    --bench submit_path --bench l2_book --bench alloc_probe \
    --bench micros --bench data --bench exec --bench signing \
    -j 1
```

`alloc_probe` is a `harness = false` bench target. Run it with
`cargo bench --bench alloc_probe`, not `cargo run --bench`.

Repeat the command five times. Report the median per case.

## submit_path

| Bench                                   | Typical | Throughput   |
| --------------------------------------- | ------- | ------------ |
| `submit_path/convert_order/market`      | 8.52 ns | 117 Melem/s  |
| `submit_path/convert_order/limit`       | 34.7 ns | 28.8 Melem/s |
| `submit_path/convert_order/stop_market` | 86.1 ns | 11.6 Melem/s |
| `submit_path/sign_request/1`            | 33.7 µs | 29.6 k/s     |
| `submit_path/sign_request/5`            | 34.5 µs | 145 k/s      |
| `submit_path/sign_request/20`           | 37.0 µs | 541 k/s      |

Signing is the fixed cost: 1 → 20 orders adds ~3.3 µs of action size, not 20×.
`sign_request/1` matches `sign_l1_action` (33.6 µs). Do not add convert + sign.

## l2

Full 20-level-per-side snapshots. `l2/apply` is Clear + N Adds (full teardown
and rebuild). `l2/full` is a composed CPU-work measurement.

| Bench          | Typical | Throughput   |
| -------------- | ------- | ------------ |
| `l2/decode/1`  | 6.04 µs | 165 k/s      |
| `l2/decode/4`  | 24.4 µs | 164 k/s      |
| `l2/convert/1` | 621 ns  | 1.61 M/s     |
| `l2/convert/4` | 2.94 µs | 1.36 M/s     |
| `l2/apply/1`   | 4.04 µs | 247 k/s      |
| `l2/apply/4`   | 16.0 µs | 249 k/s      |
| `l2/full/1`    | 11.1 µs | 90.2 k/s     |
| `l2/full/4`    | 44.8 µs | 89.2 k/s     |

`full/1` ≈ decode + convert + apply (6.04 + 0.62 + 4.04 = 10.7 µs vs 11.1).
4-instrument cases scale about 4×.

## alloc_probe

| Probe                                 | Allocs/call | Bytes/call |
| ------------------------------------- | ----------- | ---------- |
| `sign_request (1 order)`              | 17          | 778        |
| `convert_order/market`                | 0           | 0          |
| `convert_order/limit`                 | 0           | 0          |
| `convert_order/stop_market`           | 0           | 0          |
| `convert_ws_snapshot (20-level BTC)`  | 1           | 5248       |
| `apply_deltas (reused 20-level book)` | 88          | 29088      |

## Existing benches

Same session. Still CPU work only.

### Inbound pipeline (`data.rs`)

| Bench                           | Typical | Throughput |
| ------------------------------- | ------- | ---------- |
| `inbound_pipeline/book_deltas`  | 3.86 µs | 259 k/s    |
| `inbound_pipeline/book_depth10` | 3.86 µs | 259 k/s    |
| `inbound_pipeline/quotes`       | 562 ns  | 1.78 M/s   |
| `inbound_pipeline/trades`       | 678 ns  | 1.48 M/s   |
| `inbound_pipeline/mark_price`   | 1.12 µs | 895 k/s    |
| `inbound_pipeline/index_price`  | 1.12 µs | 892 k/s    |
| `inbound_pipeline/funding_rate` | 1.12 µs | 895 k/s    |
| `inbound_pipeline/bars`         | 663 ns  | 1.51 M/s   |
| `inbound_pipeline/order_event`  | 856 ns  | 1.17 M/s   |
| `inbound_pipeline/order_fill`   | 1.34 µs | 748 k/s    |

### Execution pipeline and dispatch (`exec.rs`)

| Bench                              | Typical | Throughput |
| ---------------------------------- | ------- | ---------- |
| `exec_pipeline/submit_market`      | 34.1 µs | 29.4 k/s   |
| `exec_pipeline/submit_limit`       | 34.2 µs | 29.3 k/s   |
| `exec_pipeline/submit_stop_market` | 34.5 µs | 29.0 k/s   |
| `exec_pipeline/cancel`             | 33.8 µs | 29.6 k/s   |
| `exec_pipeline/modify`             | 34.2 µs | 29.3 k/s   |
| `dispatch/fill`                    | 3.25 µs | 307 k/s    |
| `dispatch/status_accepted`         | 2.50 µs | 400 k/s    |
| `dispatch/status_canceled`         | 3.06 µs | 326 k/s    |
| `dispatch/status_modified`         | 2.92 µs | 342 k/s    |

### Signing (`signing.rs`)

| Bench                       | Typical |
| --------------------------- | ------- |
| `sign_l1_action`            | 33.6 µs |
| `sign_l1_action_with_vault` | 33.5 µs |
| `signer_construction`       | 20.4 µs |
| `msgpack_serialize_action`  | 279 ns  |
| `json_serialize_action`     | 689 ns  |

### Component breakdown (`micros.rs`)

| Bench                           | Typical |
| ------------------------------- | ------- |
| `decode_only/trade`             | 620 ns  |
| `decode_only/book`              | 3.42 µs |
| `parse_only/trade`              | 42.7 ns |
| `parse_only/book_deltas`        | 326 ns  |
| `atom/decimal_from_str`         | 3.47 ns |
| `atom/price_from_decimal_dp`    | 4.23 ns |
| `atom/price_combined`           | 7.35 ns |
| `atom/trade_id_new`             | 25.7 ns |
| `atom/uuid4_new`                | 15.2 ns |
| `atom/data_converter_ws_snapshot` | 305 ns |
| `atom/state_construct_primed`   | 2.09 µs |
| `atom/state_drop_primed`        | 515 ns  |
| `atom/event_filled_construct`   | 159 ns  |
| `atom/event_accepted_construct` | 146 ns  |
| `atom/dispatch_fill_reused`     | 9.28 ns |
