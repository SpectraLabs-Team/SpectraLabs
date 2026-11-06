// Tests for lib/paperLabEngine.ts: the engine-to-Paper-Lab mapping and the order and starter requests.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const { load, cleanup } = transpileForTest('paperlab-engine', { files: ['lib/paperLabEngine.ts'] });
after(cleanup);
const { positionFromEngine, orderFromEngine, orderRequest, starterOrders, STARTER_POSITIONS } = load('lib/paperLabEngine.js');

const pos = (over) => ({ symbol: 'AAPL', qty: 10, price: 110, avg_entry_price: 100, cost_basis: 1000, updated_at: '2026-11-06T00:00:00Z', ...over });

test('a long position shows its unrealized P&L against the average entry', () => {
  const p = positionFromEngine(pos({}));
  assert.equal(p.side, 'long');
  assert.equal(p.qty, '10');
  assert.equal(p.market_value, '1100.00');
  assert.equal(p.cost_basis, '1000.00');
  assert.equal(p.unrealized_pl, '100.00');
  assert.equal(p.unrealized_plpc, '0.1000');
});

test('a short position keeps a positive quantity and a negative P&L when price rises', () => {
  const p = positionFromEngine(pos({ qty: -10, price: 110, avg_entry_price: 100, cost_basis: -1000 }));
  assert.equal(p.side, 'short');
  assert.equal(p.qty, '10');
  assert.equal(p.unrealized_pl, '-100.00');
});

test('a position without cost basis shows zero P&L rather than a made-up one', () => {
  const p = positionFromEngine(pos({ avg_entry_price: null, cost_basis: null }));
  assert.equal(p.unrealized_pl, '0.00');
  assert.equal(p.unrealized_plpc, '0.0000');
});

test('engine order statuses map to Paper Lab statuses', () => {
  const base = { id: 'o1', workspace_id: 'w', strategy_id: 's', symbol: 'AAPL', side: 'buy', order_type: 'limit', qty: 5, tif: 'DAY', filled_qty: 0, filled_avg_price: null, created_at: 'a', updated_at: 'b' };
  assert.equal(orderFromEngine({ ...base, status: 'cancelled' }).status, 'canceled');
  assert.equal(orderFromEngine({ ...base, status: 'filled', filled_qty: 5, filled_avg_price: 101.5 }).filled_avg_price, '101.5');
  assert.equal(orderFromEngine({ ...base, status: 'rejected' }).status, 'rejected');
  assert.equal(orderFromEngine({ ...base, status: 'new', limit_price: 99 }).limit_price, '99');
  assert.equal(orderFromEngine({ ...base, status: 'new', tif: 'DAY' }).time_in_force, 'day');
});

test('market orders fill now, limit and stop orders wait', () => {
  const common = { strategyId: 'strat', symbol: 'AAPL', side: 'buy', qty: 3 };
  assert.equal(orderRequest({ ...common, type: 'market' }).fill_now, true);
  const limit = orderRequest({ ...common, type: 'limit', limitPrice: 150 });
  assert.equal(limit.fill_now, false);
  assert.equal(limit.limit_price, 150);
  assert.equal(limit.stop_price, undefined);
  assert.equal(orderRequest({ ...common, type: 'stop', stopPrice: 140 }).stop_price, 140);
});

test('stop-limit orders carry both prices and wait', () => {
  const common = { strategyId: 's', symbol: 'AAPL', side: 'buy', qty: 5 };
  const sl = orderRequest({ ...common, type: 'stop_limit', stopPrice: 140, limitPrice: 141 });
  assert.equal(sl.order_type, 'stop_limit');
  assert.equal(sl.stop_price, 140);
  assert.equal(sl.limit_price, 141);
  assert.equal(sl.fill_now, false);
});

test('stop-limit orders without a positive stop or limit price are refused', () => {
  const common = { strategyId: 's', symbol: 'AAPL', side: 'buy', qty: 5, type: 'stop_limit' };
  assert.throws(() => orderRequest({ ...common, stopPrice: 140 }), /Limit price/);
  assert.throws(() => orderRequest({ ...common, limitPrice: 141 }), /Stop price/);
  assert.throws(() => orderRequest({ ...common, stopPrice: 0, limitPrice: 141 }), /Stop price/);
});

test('orders for zero or non-numeric quantity are refused before they are sent', () => {
  assert.throws(() => orderRequest({ strategyId: 's', symbol: 'AAPL', side: 'buy', type: 'market', qty: 0 }), /positive/);
  assert.throws(() => orderRequest({ strategyId: 's', symbol: 'AAPL', side: 'buy', type: 'market', qty: NaN }), /positive/);
});

test('the starter book is AAPL 50 and NVDA 20, bought at the dataset close', () => {
  assert.deepEqual(STARTER_POSITIONS.map((p) => [p.symbol, p.qty]), [['AAPL', 50], ['NVDA', 20]]);
  const orders = starterOrders('strat');
  assert.equal(orders.length, 2);
  for (const o of orders) {
    assert.equal(o.side, 'buy');
    assert.equal(o.order_type, 'market');
    assert.equal(o.fill_now, true);
    assert.equal(o.fill_price, undefined, 'no client price: the engine prices the fill');
  }
});
