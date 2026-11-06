/**
 * Paper Lab on the engine (M11 Task 2). Pure mapping between the engine's paper account and
 * order views and the Alpaca-shaped rows Paper Lab draws, plus the starter book. No I/O here, so
 * the rules can be tested without a server.
 */
import type { AlpacaOrder, AlpacaPosition } from './alpaca';
import type { CreatePaperOrderRequest, PaperOrderView, PaperPositionView } from './api';

/** The book every new Paper Lab account starts with. Seeded as market fills at the dataset close. */
export const STARTER_POSITIONS: ReadonlyArray<{ symbol: string; qty: number }> = [
  { symbol: 'AAPL', qty: 50 },
  { symbol: 'NVDA', qty: 20 },
];

const money = (n: number) => n.toFixed(2);

/** An engine position drawn as Paper Lab's position row. P&L is unrealized against the average entry. */
export function positionFromEngine(p: PaperPositionView): AlpacaPosition {
  const side: 'long' | 'short' = p.qty < 0 ? 'short' : 'long';
  const marketValue = p.qty * p.price;
  const costBasis = p.cost_basis ?? (p.avg_entry_price != null ? p.avg_entry_price * p.qty : null);
  const avg = p.avg_entry_price ?? null;
  const unrealized = avg != null ? (p.price - avg) * p.qty : 0;
  const basis = costBasis != null ? Math.abs(costBasis) : 0;
  return {
    asset_id: `pl-${p.symbol}`,
    symbol: p.symbol,
    exchange: 'NASDAQ',
    asset_class: 'us_equity',
    avg_entry_price: avg != null ? money(avg) : money(p.price),
    qty: String(Math.abs(p.qty)),
    side,
    market_value: money(marketValue),
    cost_basis: money(costBasis ?? p.price * p.qty),
    unrealized_pl: money(unrealized),
    unrealized_plpc: basis > 0 ? (unrealized / basis).toFixed(4) : '0.0000',
    current_price: String(p.price),
  };
}

const STATUS: Record<PaperOrderView['status'], AlpacaOrder['status']> = {
  new: 'new',
  partially_filled: 'partially_filled',
  filled: 'filled',
  cancelled: 'canceled',
  rejected: 'rejected',
};

/** An engine order drawn as a Paper Lab order row. */
export function orderFromEngine(o: PaperOrderView): AlpacaOrder {
  return {
    id: o.id,
    client_order_id: o.id,
    created_at: o.created_at,
    updated_at: o.updated_at,
    symbol: o.symbol,
    qty: String(o.qty),
    filled_qty: String(o.filled_qty),
    filled_avg_price: o.filled_avg_price != null ? String(o.filled_avg_price) : null,
    type: o.order_type,
    side: o.side,
    time_in_force: (o.tif || 'day').toLowerCase() as AlpacaOrder['time_in_force'],
    limit_price: o.limit_price != null ? String(o.limit_price) : null,
    stop_price: o.stop_price != null ? String(o.stop_price) : null,
    status: STATUS[o.status],
  };
}

export interface TradeInput {
  strategyId: string;
  symbol: string;
  side: 'buy' | 'sell';
  type: 'market' | 'limit' | 'stop';
  qty: number;
  limitPrice?: number;
  stopPrice?: number;
}

/** The engine request for a trade. Market orders fill now at the dataset close; others wait. */
export function orderRequest(t: TradeInput): CreatePaperOrderRequest {
  if (!(t.qty > 0) || !Number.isFinite(t.qty)) throw new Error('Quantity must be a positive number.');
  return {
    strategy_id: t.strategyId,
    symbol: t.symbol,
    side: t.side,
    order_type: t.type,
    qty: t.qty,
    limit_price: t.type === 'limit' ? t.limitPrice : undefined,
    stop_price: t.type === 'stop' ? t.stopPrice : undefined,
    fill_now: t.type === 'market',
  };
}

/** The requests that seed a new account with the starter book. */
export function starterOrders(strategyId: string): CreatePaperOrderRequest[] {
  return STARTER_POSITIONS.map((p) => ({
    strategy_id: strategyId,
    symbol: p.symbol,
    side: 'buy',
    order_type: 'market',
    qty: p.qty,
    fill_now: true,
  }));
}
