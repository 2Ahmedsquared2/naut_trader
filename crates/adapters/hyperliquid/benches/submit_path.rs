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

//! CPU-work benches for the Hyperliquid order-submit conversion and signing
//! steps.
//!
//! `submit_path/convert_order` times
//! `order_to_hyperliquid_request_with_asset_and_cloid` on a single order
//! (`cloid = None`, matching execution.rs:790). Parameterized over
//! `{market, limit, stop_market}`. Production converts a batch with a plain
//! per-order loop (websocket/client.rs:843-863); there is no batch conversion
//! function, so a batch case would measure exactly N times a single convert
//! and is omitted.
//!
//! `submit_path/sign_request` times
//! `HyperliquidHttpClient::sign_action_exec_request(&action, None)`, which is
//! exactly what production calls from websocket/client.rs:650. Parameterized
//! over `{1, 5, 20}` orders inside one `HyperliquidExchangeAction::Order`.
//! That is real production batching: one msgpack, one keccak, one ECDSA
//! signature regardless of N (websocket/client.rs:872-876).
//! Throughput is `Elements(n)` so Criterion reports the per-order cost as the
//! fixed signing cost amortizes.
//!
//! These two benches MUST NOT be summed. They measure different functions on
//! different inputs; convert is per-order and sign is per-action.
//!
//! None of these numbers is production latency. They are CPU work only.
//!
//! Excluded: `ExecutionClient::submit_order`. It returns immediately after
//! `task_spawner.spawn` (execution.rs:869) with no completion signal.
//! Benchmarking it would spawn one task per iteration that performs cache
//! writes, context registration, event emission, and a full EIP-712 signature
//! before failing at transport. Thousands of those run concurrently on
//! runtime threads, grow the cloid cache and dispatch state without bound,
//! and fill an undrained event channel — contaminating later samples and
//! leaking. It is deferred to a later mock-WebSocket benchmark.
//!
//! Consequence: nothing here measures async scheduling, the PostRouter,
//! channel hops, handler serialization, or the socket write.

mod common;

use std::hint::black_box;

use common::{
    BTC_ASSET_INDEX, DEFAULT_SLIPPAGE_BPS, PRICE_DECIMALS, limit_order, market_order,
    signed_http_client, stop_market_order,
};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_hyperliquid::{
    common::parse::order_to_hyperliquid_request_with_asset_and_cloid,
    http::models::{HyperliquidExchangeAction, HyperliquidExchangeGrouping},
};
use nautilus_model::enums::OrderSide;

fn bench_convert_order(c: &mut Criterion) {
    let cases = [
        ("convert_order/market", market_order(OrderSide::Buy)),
        ("convert_order/limit", limit_order(OrderSide::Buy)),
        (
            "convert_order/stop_market",
            stop_market_order(OrderSide::Sell),
        ),
    ];

    let mut group = c.benchmark_group("submit_path");
    group.throughput(Throughput::Elements(1));

    for (id, order) in cases {
        group.bench_function(id, |b| {
            b.iter(|| {
                let req = order_to_hyperliquid_request_with_asset_and_cloid(
                    black_box(&order),
                    BTC_ASSET_INDEX,
                    PRICE_DECIMALS,
                    true,
                    DEFAULT_SLIPPAGE_BPS,
                    None,
                )
                .unwrap();
                black_box(req);
            });
        });
    }
    group.finish();
}

fn bench_sign_request(c: &mut Criterion) {
    let client = signed_http_client();
    let order = limit_order(OrderSide::Buy);
    let req = order_to_hyperliquid_request_with_asset_and_cloid(
        &order,
        BTC_ASSET_INDEX,
        PRICE_DECIMALS,
        true,
        DEFAULT_SLIPPAGE_BPS,
        None,
    )
    .unwrap();

    let mut group = c.benchmark_group("submit_path");

    for n in [1usize, 5, 20] {
        group.throughput(Throughput::Elements(n as u64));
        let action = HyperliquidExchangeAction::Order {
            orders: vec![req.clone(); n],
            grouping: HyperliquidExchangeGrouping::Na,
            builder: None,
        };
        group.bench_with_input(BenchmarkId::new("sign_request", n), &action, |b, action| {
            b.iter(|| {
                let signed = client
                    .sign_action_exec_request(black_box(action), None)
                    .unwrap();
                black_box(signed);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_convert_order, bench_sign_request);
criterion_main!(benches);
