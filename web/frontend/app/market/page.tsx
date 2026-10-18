export default function MarketPage() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-bold">Market Data</h1>
      <p className="text-foreground-muted mt-2">
        Brokerage market data is not connected in this build (the Alpaca proxy has not been ported to the Rust API).
      </p>
    </div>
  );
}
