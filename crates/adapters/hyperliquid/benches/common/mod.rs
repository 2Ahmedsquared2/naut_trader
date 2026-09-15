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

//! Shared utilities for hyperliquid criterion benches.
//!
//! Fixtures live as inline `&'static str` consts to keep benches deterministic
//! and self-contained; real venue captures in `test_data/` are reserved for
//! parser correctness tests. The L2 benches use generated 20-level-per-side
//! frames from [`l2_book_20_json`] rather than the 10-level [`fixtures::BOOK_L2`].
//!
//! Each criterion bench is a separate compilation unit that pulls in this
//! module, but uses only a subset of the benchmark routines and fixtures. Without the
//! module-level `allow`, the unused subset in any given bench triggers
//! per-crate dead-code warnings.

#![allow(dead_code)]

use std::{fmt::Write, str::FromStr};

use ahash::AHashMap;
use nautilus_common::{cache::Cache, messages::ExecutionEvent};
use nautilus_core::{
    AtomicTime, UUID4, UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_hyperliquid::{
    HyperliquidHttpClient,
    common::{consts::HYPERLIQUID_VENUE, enums::HyperliquidEnvironment},
};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{AccountType, OrderSide, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny},
    orders::{LimitOrder, MarketOrder, OrderAny, StopMarketOrder},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

pub(crate) const TRADER_ID: &str = "BENCH-001";
pub(crate) const ACCOUNT_ID: &str = "HYPERLIQUID-001";
pub(crate) const TEST_KEY: &str =
    "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
pub(crate) const BTC_ASSET_INDEX: u32 = 3;
pub(crate) const PRICE_DECIMALS: u8 = 2;
pub(crate) const DEFAULT_SLIPPAGE_BPS: u32 = 50;
pub(crate) const L2_LEVELS_PER_SIDE: usize = 20;
pub(crate) const L2_COINS: &[&str] = &["BTC", "ETH", "SOL", "ARB"];

#[must_use]
pub(crate) fn clock() -> &'static AtomicTime {
    get_atomic_clock_realtime()
}

#[must_use]
pub(crate) fn trader_id() -> TraderId {
    TraderId::from(TRADER_ID)
}

#[must_use]
pub(crate) fn account_id() -> AccountId {
    AccountId::from(ACCOUNT_ID)
}

#[must_use]
pub(crate) fn btc_perp() -> InstrumentAny {
    perp_instrument("BTC", 2, 4)
}

#[must_use]
pub(crate) fn eth_perp() -> InstrumentAny {
    perp_instrument("ETH", 2, 4)
}

#[must_use]
pub(crate) fn sol_perp() -> InstrumentAny {
    perp_instrument("SOL", 2, 4)
}

#[must_use]
pub(crate) fn arb_perp() -> InstrumentAny {
    perp_instrument("ARB", 4, 4)
}

fn perp_instrument(coin: &str, price_precision: u8, size_precision: u8) -> InstrumentAny {
    let symbol_str = format!("{coin}-USD-PERP");
    let raw_symbol = Symbol::new(coin);
    let instrument_id = InstrumentId::new(Symbol::new(&symbol_str), *HYPERLIQUID_VENUE);
    let price_increment = Price::new(10f64.powi(-(price_precision as i32)), price_precision);
    let size_increment = Quantity::new(10f64.powi(-(size_precision as i32)), size_precision);
    InstrumentAny::CryptoPerpetual(
        CryptoPerpetual::builder()
            .instrument_id(instrument_id)
            .raw_symbol(raw_symbol)
            .base_currency(Currency::from(coin))
            .quote_currency(Currency::from("USDC"))
            .settlement_currency(Currency::from("USDC"))
            .is_inverse(false)
            .price_precision(price_precision)
            .size_precision(size_precision)
            .price_increment(price_increment)
            .size_increment(size_increment)
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

#[must_use]
pub(crate) fn instrument_cache() -> AHashMap<Ustr, InstrumentAny> {
    let mut cache = AHashMap::new();
    let btc = btc_perp();
    let eth = eth_perp();
    cache.insert(Ustr::from("BTC"), btc);
    cache.insert(Ustr::from("ETH"), eth);
    cache
}

/// Builds an [`ExecutionEventEmitter`] connected to an unbounded channel whose
/// receiver is returned alongside the emitter; tests/benches must keep the
/// receiver alive (drop closes the channel and turns `send_order_event` into a
/// warn-logging no-op which skews the measurement).
#[must_use]
pub(crate) fn bench_emitter() -> (
    ExecutionEventEmitter,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let mut emitter = ExecutionEventEmitter::new(
        clock(),
        trader_id(),
        account_id(),
        AccountType::Margin,
        Some(Currency::from("USDC")),
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    (emitter, rx)
}

/// Convenience cache (used by future dispatch benches that need it).
#[must_use]
pub(crate) fn empty_cache() -> Cache {
    Cache::default()
}

#[must_use]
pub(crate) fn strategy_id() -> StrategyId {
    StrategyId::from("S-BENCH")
}

#[must_use]
pub(crate) fn client_order_id(suffix: &str) -> ClientOrderId {
    ClientOrderId::from(format!("O-BENCH-{suffix}").as_str())
}

#[must_use]
pub(crate) fn limit_order(side: OrderSide) -> OrderAny {
    OrderAny::Limit(LimitOrder::new(
        trader_id(),
        strategy_id(),
        btc_perp().id(),
        client_order_id("LIM"),
        side,
        Quantity::from("0.001"),
        Price::from("92572.0"),
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ))
}

/// Constructs a market order the way production conversion receives it.
///
/// `MarketOrder` has no limit price, so
/// `order_to_hyperliquid_request_with_asset_and_cloid` writes `Decimal::ZERO`.
/// Production then overlays `derive_market_order_price` from a cached quote
/// inside `submit_order` (execution.rs:826), which is excluded from these
/// benches and must not be reimplemented here.
#[must_use]
pub(crate) fn market_order(side: OrderSide) -> OrderAny {
    OrderAny::Market(MarketOrder::new(
        trader_id(),
        strategy_id(),
        btc_perp().id(),
        client_order_id("MKT"),
        side,
        Quantity::from("0.001"),
        TimeInForce::Ioc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
}

#[must_use]
pub(crate) fn stop_market_order(side: OrderSide) -> OrderAny {
    OrderAny::StopMarket(StopMarketOrder::new(
        trader_id(),
        strategy_id(),
        btc_perp().id(),
        client_order_id("STP"),
        side,
        Quantity::from("0.001"),
        Price::from("90000.0"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ))
}

/// Authenticated HTTP client used only so benches can call
/// [`HyperliquidHttpClient::sign_action_exec_request`].
#[must_use]
pub(crate) fn signed_http_client() -> HyperliquidHttpClient {
    HyperliquidHttpClient::from_credentials(
        TEST_KEY,
        None,
        HyperliquidEnvironment::Mainnet,
        60,
        None,
    )
    .expect("bench fixture private key must construct a signer")
}

#[must_use]
pub(crate) fn l2_instrument(coin: &str) -> InstrumentAny {
    match coin {
        "BTC" => btc_perp(),
        "ETH" => eth_perp(),
        "SOL" => sol_perp(),
        "ARB" => arb_perp(),
        other => panic!("unknown L2 bench coin: {other}"),
    }
}

#[must_use]
pub(crate) fn l2_best_bid_tick(coin: &str) -> (&'static str, &'static str) {
    match coin {
        "BTC" => ("98450.5", "0.5"),
        "ETH" => ("2114.25", "0.05"),
        "SOL" => ("94.88", "0.01"),
        "ARB" => ("0.3524", "0.0001"),
        other => panic!("unknown L2 bench coin: {other}"),
    }
}

#[must_use]
pub(crate) fn l2_json_for_coin(coin: &str) -> String {
    let (best_bid, tick) = l2_best_bid_tick(coin);
    l2_book_20_json(coin, best_bid, tick)
}

/// Builds a venue-shaped `l2Book` JSON frame with 20 levels per side.
///
/// `best_bid` and `tick` are decimal strings. Bids descend from `best_bid` by
/// `tick`; asks ascend from `best_bid + tick` by `tick`. Sizes vary slightly by
/// level so the payload is not a repeated constant.
#[must_use]
pub(crate) fn l2_book_20_json(coin: &str, best_bid: &str, tick: &str) -> String {
    let best_bid = Decimal::from_str(best_bid).expect("best_bid must be a decimal");
    let tick = Decimal::from_str(tick).expect("tick must be a decimal");
    let mut bids = String::new();
    let mut asks = String::new();

    for i in 0..L2_LEVELS_PER_SIDE {
        if i > 0 {
            bids.push(',');
            asks.push(',');
        }
        let bid_px = best_bid - tick * Decimal::from(i as u64);
        let ask_px = best_bid + tick * Decimal::from((i + 1) as u64);
        let bid_sz = Decimal::new(25 + i as i64 * 3, 1);
        let ask_sz = Decimal::new(15 + i as i64 * 2, 1);
        let n = (i % 4) + 1;
        write!(
            bids,
            r#"{{"px":"{}","sz":"{}","n":{}}}"#,
            bid_px.normalize(),
            bid_sz.normalize(),
            n,
        )
        .expect("writing to String is infallible");
        write!(
            asks,
            r#"{{"px":"{}","sz":"{}","n":{}}}"#,
            ask_px.normalize(),
            ask_sz.normalize(),
            n,
        )
        .expect("writing to String is infallible");
    }
    let mut json = String::new();
    json.push_str(r#"{"channel":"l2Book","data":{"coin":""#);
    json.push_str(coin);
    json.push_str(r#"","levels":[["#);
    json.push_str(&bids);
    json.push_str("],[");
    json.push_str(&asks);
    json.push_str(r#"]],"time":1733833200000}}"#);
    json
}

pub(crate) mod fixtures {
    //! Inline WS frame strings shaped exactly like the venue wire format. Each
    //! fixture exercises one [`HyperliquidWsMessage`] variant end-to-end and is
    //! kept small enough to be obvious at a glance.

    pub(crate) const TRADE: &str = r#"{
        "channel": "trades",
        "data": [{
            "coin": "BTC",
            "side": "B",
            "px": "98450.5",
            "sz": "0.0123",
            "hash": "0xabc123",
            "time": 1733833200000,
            "tid": 987654321,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }]
    }"#;

    pub(crate) const BOOK_L2: &str = r#"{
        "channel": "l2Book",
        "data": {
            "coin": "BTC",
            "levels": [
                [
                    {"px": "98450.5", "sz": "2.5", "n": 3},
                    {"px": "98449.0", "sz": "1.8", "n": 2},
                    {"px": "98448.0", "sz": "0.75", "n": 1},
                    {"px": "98447.0", "sz": "3.2", "n": 4},
                    {"px": "98446.0", "sz": "1.1", "n": 2},
                    {"px": "98445.0", "sz": "2.0", "n": 3},
                    {"px": "98444.0", "sz": "0.5", "n": 1},
                    {"px": "98443.0", "sz": "1.4", "n": 2},
                    {"px": "98442.0", "sz": "0.9", "n": 1},
                    {"px": "98441.0", "sz": "1.7", "n": 2}
                ],
                [
                    {"px": "98451.0", "sz": "1.5", "n": 2},
                    {"px": "98452.0", "sz": "2.1", "n": 3},
                    {"px": "98453.0", "sz": "0.9", "n": 1},
                    {"px": "98454.0", "sz": "1.7", "n": 2},
                    {"px": "98455.0", "sz": "2.3", "n": 4},
                    {"px": "98456.0", "sz": "0.6", "n": 1},
                    {"px": "98457.0", "sz": "1.2", "n": 2},
                    {"px": "98458.0", "sz": "0.8", "n": 1},
                    {"px": "98459.0", "sz": "1.5", "n": 2},
                    {"px": "98460.0", "sz": "2.0", "n": 3}
                ]
            ],
            "time": 1733833200000
        }
    }"#;

    pub(crate) const BBO: &str = r#"{
        "channel": "bbo",
        "data": {
            "coin": "BTC",
            "time": 1733833200000,
            "bbo": [
                {"px": "98450.5", "sz": "2.5", "n": 3},
                {"px": "98451.0", "sz": "1.5", "n": 2}
            ]
        }
    }"#;

    pub(crate) const CANDLE: &str = r#"{
        "channel": "candle",
        "data": {
            "t": 1733833200000,
            "T": 1733833260000,
            "s": "BTC",
            "i": "1m",
            "o": "98450.0",
            "c": "98460.0",
            "h": "98470.0",
            "l": "98440.0",
            "v": "10.5",
            "n": 42
        }
    }"#;

    pub(crate) const ALL_MIDS: &str = r#"{
        "channel": "allMids",
        "data": {
            "mids": {
                "BTC": "98455.5",
                "ETH": "2114.25",
                "SOL": "94.88"
            }
        }
    }"#;

    pub(crate) const ACTIVE_ASSET_CTX_PERP: &str = r#"{
        "channel": "activeAssetCtx",
        "data": {
            "coin": "BTC",
            "ctx": {
                "dayNtlVlm": "1000000.0",
                "prevDayPx": "97000.0",
                "markPx": "98455.5",
                "midPx": "98455.0",
                "impactPxs": ["98454.0", "98456.0"],
                "dayBaseVlm": "100.0",
                "funding": "0.0001",
                "openInterest": "1500.0",
                "oraclePx": "98460.0",
                "premium": "-0.0001"
            }
        }
    }"#;

    pub(crate) const ORDER_UPDATE: &str = r#"{
        "channel": "orderUpdates",
        "data": [{
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "98000.0",
                "sz": "0.5",
                "oid": 430481837807,
                "timestamp": 1733833200000,
                "origSz": "1.0",
                "cloid": "0xd211f1c27288259290850338d22132a0"
            },
            "status": "open",
            "statusTimestamp": 1733833200000
        }]
    }"#;

    pub(crate) const USER_FILL: &str = r#"{
        "channel": "user",
        "data": {
            "fills": [{
                "coin": "BTC",
                "px": "98450.5",
                "sz": "0.1",
                "side": "B",
                "time": 1733833200000,
                "startPosition": "0.0",
                "dir": "Open Long",
                "closedPnl": "0.0",
                "hash": "0xabc123",
                "oid": 430481837807,
                "crossed": true,
                "fee": "0.05",
                "tid": 98765,
                "feeToken": "USDC",
                "cloid": "0xd211f1c27288259290850338d22132a0"
            }]
        }
    }"#;
}
