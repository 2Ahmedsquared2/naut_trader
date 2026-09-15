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

//! Allocation probe for Hyperliquid CPU-work entry points.
//!
//! This is a bench target (`harness = false`), not production code, so a
//! file-local `#[global_allocator]` does not violate AGENTS.md (that rule
//! forbids test-only behavior on production `src/`). The probe is a separate
//! binary because Criterion's harness allocates and would pollute counts, and
//! because only one global allocator is permitted per binary.
//!
//! Run with `cargo bench -p nautilus-hyperliquid --bench alloc_probe`.
//! `cargo run` has no `--bench` flag.
//!
//! Counts allocations and bytes for:
//! 1. `HyperliquidHttpClient::sign_action_exec_request`
//! 2. `order_to_hyperliquid_request_with_asset_and_cloid`
//! 4. `HyperliquidDataConverter::convert_ws_snapshot`
//! 5. `OrderBook::apply_deltas`
//!
//! Numbers are CPU-work allocations, not production latency.

mod common;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering},
};

use common::{
    BTC_ASSET_INDEX, DEFAULT_SLIPPAGE_BPS, PRICE_DECIMALS, l2_instrument, l2_json_for_coin,
    limit_order, market_order, signed_http_client, stop_market_order,
};
use nautilus_core::UnixNanos;
use nautilus_hyperliquid::{
    common::{
        HyperliquidDataConverter, parse::order_to_hyperliquid_request_with_asset_and_cloid,
    },
    http::models::{HyperliquidExchangeAction, HyperliquidExchangeGrouping},
    websocket::messages::HyperliquidWsMessage,
};
use nautilus_model::{
    enums::{BookType, OrderSide},
    instruments::Instrument,
    orderbook::OrderBook,
};

struct CountingAllocator;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

fn record(size: usize) {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(size as u64, Ordering::Relaxed);
}

// Forwards to `System` with the same pointer and layout the caller supplied.
// Counters are relaxed atomics and do not affect allocation correctness.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: forwarding the caller's layout to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: forwarding the caller's layout to the system allocator.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        // SAFETY: `ptr` was allocated with `layout` by this allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` was allocated with `layout` by this allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const ITERS: u64 = 1_000;
const WARMUP: u64 = 16;

fn reset() {
    ALLOCS.store(0, Ordering::SeqCst);
    BYTES.store(0, Ordering::SeqCst);
}

fn snapshot() -> (u64, u64) {
    (
        ALLOCS.load(Ordering::SeqCst),
        BYTES.load(Ordering::SeqCst),
    )
}

fn probe(name: &str, mut body: impl FnMut()) {
    for _ in 0..WARMUP {
        body();
    }
    reset();

    for _ in 0..ITERS {
        body();
    }
    let (allocs, bytes) = snapshot();
    println!("{name}");
    println!(
        "  allocs: {allocs} ({:.2}/call)",
        allocs as f64 / ITERS as f64
    );
    println!(
        "  bytes:  {bytes} ({:.2}/call)",
        bytes as f64 / ITERS as f64
    );
}

fn main() {
    println!("alloc_probe (counting GlobalAlloc in this binary only)");
    println!("iterations: {ITERS} (warmup {WARMUP})");
    println!("none of these numbers is production latency");
    println!();

    let client = signed_http_client();
    let limit = limit_order(OrderSide::Buy);
    let market = market_order(OrderSide::Buy);
    let stop = stop_market_order(OrderSide::Sell);
    let req = order_to_hyperliquid_request_with_asset_and_cloid(
        &limit,
        BTC_ASSET_INDEX,
        PRICE_DECIMALS,
        true,
        DEFAULT_SLIPPAGE_BPS,
        None,
    )
    .unwrap();
    let action = HyperliquidExchangeAction::Order {
        orders: vec![req],
        grouping: HyperliquidExchangeGrouping::Na,
        builder: None,
    };

    probe("sign_request (1 order)", || {
        let signed = client
            .sign_action_exec_request(black_box(&action), None)
            .unwrap();
        black_box(signed);
    });

    probe("convert_order/market", || {
        let converted = order_to_hyperliquid_request_with_asset_and_cloid(
            black_box(&market),
            BTC_ASSET_INDEX,
            PRICE_DECIMALS,
            true,
            DEFAULT_SLIPPAGE_BPS,
            None,
        )
        .unwrap();
        black_box(converted);
    });

    probe("convert_order/limit", || {
        let converted = order_to_hyperliquid_request_with_asset_and_cloid(
            black_box(&limit),
            BTC_ASSET_INDEX,
            PRICE_DECIMALS,
            true,
            DEFAULT_SLIPPAGE_BPS,
            None,
        )
        .unwrap();
        black_box(converted);
    });

    probe("convert_order/stop_market", || {
        let converted = order_to_hyperliquid_request_with_asset_and_cloid(
            black_box(&stop),
            BTC_ASSET_INDEX,
            PRICE_DECIMALS,
            true,
            DEFAULT_SLIPPAGE_BPS,
            None,
        )
        .unwrap();
        black_box(converted);
    });

    let converter = HyperliquidDataConverter::new();
    let instrument_id = l2_instrument("BTC").id();
    let json = l2_json_for_coin("BTC");
    let msg: HyperliquidWsMessage = serde_json::from_str(&json).unwrap();
    let HyperliquidWsMessage::L2Book { data } = msg else {
        unreachable!("l2 fixture must decode as L2Book")
    };
    let ts_init = UnixNanos::default();
    let deltas = converter
        .convert_ws_snapshot(&data, instrument_id, ts_init)
        .unwrap();

    probe("convert_ws_snapshot (20-level BTC)", || {
        let converted = converter
            .convert_ws_snapshot(black_box(&data), instrument_id, ts_init)
            .unwrap();
        black_box(converted);
    });

    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    book.apply_deltas(&deltas).unwrap();
    probe("apply_deltas (reused 20-level book)", || {
        book.apply_deltas(black_box(&deltas)).unwrap();
    });
}
