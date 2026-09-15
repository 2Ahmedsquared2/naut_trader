// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! CPU-work benches for Hyperliquid L2 book ingest.
//!
//! Fixtures are full 20-level-per-side `l2Book` frames, never BBO.
//! Parameterized over 1 and 4 instruments.
//!
//! - `l2/decode` times `serde_json::from_str::<HyperliquidWsMessage>` (the
//!   real call site is handler.rs:528)
//! - `l2/convert` times `HyperliquidDataConverter::convert_ws_snapshot`
//! - `l2/apply` times `OrderBook::apply_deltas` on a reused, long-lived book
//!   (production applies to a persistent book). Book construction is outside
//!   the timed region via `iter_batched_ref`
//! - `l2/full` is decode + convert + apply in one timed region. It is a
//!   composed CPU-work measurement, not production latency
//!
//! `convert_ws_snapshot` emits Clear + N Adds (asserted by the tests in
//! `common/models.rs`), so `l2/apply` measures a full book teardown and
//! rebuild. These benches do not implement snapshot diffing.
//!
//! None of these numbers is production latency. They include no I/O, no async
//! runtime, and no channel hop.

mod common;

use std::hint::black_box;

use common::{L2_COINS, l2_instrument, l2_json_for_coin};
use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use nautilus_core::UnixNanos;
use nautilus_hyperliquid::{
    common::HyperliquidDataConverter,
    websocket::messages::{HyperliquidWsMessage, WsBookData},
};
use nautilus_model::{
    data::OrderBookDeltas,
    enums::{BookAction, BookType},
    identifiers::InstrumentId,
    instruments::Instrument,
    orderbook::OrderBook,
};

struct L2Case {
    json: String,
    instrument_id: InstrumentId,
    book_data: WsBookData,
    deltas: OrderBookDeltas,
}

fn l2_cases(n: usize) -> Vec<L2Case> {
    let converter = HyperliquidDataConverter::new();
    let ts_init = UnixNanos::default();
    L2_COINS[..n]
        .iter()
        .map(|coin| {
            let json = l2_json_for_coin(coin);
            let instrument_id = l2_instrument(coin).id();
            let msg: HyperliquidWsMessage = serde_json::from_str(&json).unwrap();
            let HyperliquidWsMessage::L2Book { data } = msg else {
                unreachable!("l2 fixture must decode as L2Book")
            };
            let deltas = converter
                .convert_ws_snapshot(&data, instrument_id, ts_init)
                .unwrap();
            L2Case {
                json,
                instrument_id,
                book_data: data,
                deltas,
            }
        })
        .collect()
}

fn assert_snapshot_shape(cases: &[L2Case]) {
    for case in cases {
        assert_eq!(case.deltas.deltas[0].action, BookAction::Clear);
        assert!(
            case.deltas.deltas[1..]
                .iter()
                .all(|delta| delta.action == BookAction::Add)
        );
    }
}

fn primed_books(cases: &[L2Case]) -> Vec<OrderBook> {
    cases
        .iter()
        .map(|case| {
            let mut book = OrderBook::new(case.instrument_id, BookType::L2_MBP);
            book.apply_deltas(&case.deltas).unwrap();
            book
        })
        .collect()
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("l2");

    for n in [1usize, 4] {
        let cases = l2_cases(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("decode", n), &cases, |b, cases| {
            b.iter(|| {
                for case in cases {
                    let msg: HyperliquidWsMessage =
                        serde_json::from_str(black_box(case.json.as_str())).unwrap();
                    black_box(msg);
                }
            });
        });
    }
    group.finish();
}

fn bench_convert(c: &mut Criterion) {
    let converter = HyperliquidDataConverter::new();
    let ts_init = UnixNanos::default();
    let mut group = c.benchmark_group("l2");

    for n in [1usize, 4] {
        let cases = l2_cases(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("convert", n), &cases, |b, cases| {
            b.iter(|| {
                for case in cases {
                    let deltas = converter
                        .convert_ws_snapshot(
                            black_box(&case.book_data),
                            case.instrument_id,
                            ts_init,
                        )
                        .unwrap();
                    black_box(deltas);
                }
            });
        });
    }
    group.finish();
}

fn bench_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("l2");

    for n in [1usize, 4] {
        let cases = l2_cases(n);
        assert_snapshot_shape(&cases);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("apply", n), &cases, |b, cases| {
            b.iter_batched_ref(
                || primed_books(cases),
                |books| {
                    for (book, case) in books.iter_mut().zip(cases) {
                        book.apply_deltas(black_box(&case.deltas)).unwrap();
                    }
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_full(c: &mut Criterion) {
    let converter = HyperliquidDataConverter::new();
    let ts_init = UnixNanos::default();
    let mut group = c.benchmark_group("l2");

    for n in [1usize, 4] {
        let cases = l2_cases(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("full", n), &cases, |b, cases| {
            b.iter_batched_ref(
                || primed_books(cases),
                |books| {
                    for (book, case) in books.iter_mut().zip(cases) {
                        let msg: HyperliquidWsMessage =
                            serde_json::from_str(black_box(case.json.as_str())).unwrap();
                        let HyperliquidWsMessage::L2Book { data } = msg else {
                            unreachable!("l2 fixture must decode as L2Book")
                        };
                        let deltas = converter
                            .convert_ws_snapshot(&data, case.instrument_id, ts_init)
                            .unwrap();
                        book.apply_deltas(&deltas).unwrap();
                    }
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_decode, bench_convert, bench_apply, bench_full);
criterion_main!(benches);
