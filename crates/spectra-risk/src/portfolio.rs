//! Portfolios: a plain list of positions, loaded from code or CSV.
//!
//! CSV format: a header row, then `symbol,quantity,price[,volatility]`.
//! Blank lines are skipped. The optional fourth column is the asset's
//! annualized volatility (0.25 = 25%); assets without one use the model's
//! default. (The C++ loader documented the fourth column but ignored it, so
//! every asset silently got the default; it is read here.)

use spectra_core::{Error, Result};

/// One position.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    /// Ticker or identifier.
    pub symbol: String,
    /// Units held; negative for a short position.
    pub quantity: f64,
    /// Last known price.
    pub price: f64,
    /// Annualized volatility; 0 means "use the model default".
    pub volatility: f64,
}

impl Asset {
    /// Position value (`quantity * price`).
    pub fn value(&self) -> f64 {
        self.quantity * self.price
    }
}

/// A portfolio. Knows nothing about simulation or risk.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Portfolio {
    assets: Vec<Asset>,
}

fn data_err(msg: impl Into<String>) -> Error {
    Error::Data(msg.into())
}

fn parse_number(field: &str) -> Option<f64> {
    let v: f64 = field.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

impl Portfolio {
    /// An empty portfolio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a position with the default volatility.
    pub fn add_asset(&mut self, symbol: impl Into<String>, quantity: f64, price: f64) -> &mut Self {
        self.add_asset_with_volatility(symbol, quantity, price, 0.0)
    }

    /// Adds a position with its own annualized volatility.
    pub fn add_asset_with_volatility(
        &mut self,
        symbol: impl Into<String>,
        quantity: f64,
        price: f64,
        volatility: f64,
    ) -> &mut Self {
        self.assets.push(Asset { symbol: symbol.into(), quantity, price, volatility });
        self
    }

    /// Positions in insertion order.
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    /// Number of positions.
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Whether there are no positions.
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Sum of position values (summed in insertion order, like the C++ engine).
    pub fn total_value(&self) -> f64 {
        self.assets.iter().fold(0.0, |acc, a| acc + a.quantity * a.price)
    }

    /// Each position's share of gross exposure (`|value| / sum |value|`).
    /// Empty if the portfolio has no exposure.
    pub fn gross_weights(&self) -> Vec<f64> {
        let gross: f64 = self.assets.iter().map(|a| a.value().abs()).sum();
        if gross == 0.0 {
            return Vec::new();
        }
        self.assets.iter().map(|a| a.value().abs() / gross).collect()
    }

    /// Herfindahl-Hirschman concentration index of the gross weights, in
    /// [1/n, 1]: 1 means everything is in one position. (The C++ engine
    /// returned a hard-coded 1.0.)
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if the portfolio has no exposure.
    pub fn hhi(&self) -> Result<f64> {
        let w = self.gross_weights();
        if w.is_empty() {
            return Err(Error::InvalidArgument("HHI is undefined for a portfolio with no exposure.".into()));
        }
        Ok(w.iter().map(|x| x * x).sum())
    }

    /// The same positions with every price multiplied by `factor`.
    #[must_use]
    pub fn shocked(&self, factor: f64) -> Self {
        Self { assets: self.assets.iter().map(|a| Asset { price: a.price * factor, ..a.clone() }).collect() }
    }

    /// Parses CSV text (see the module documentation).
    ///
    /// # Errors
    /// [`Error::Data`] for a row with fewer than three fields or a field that
    /// is not a finite number; messages match the C++ loader.
    pub fn from_csv_str(text: &str) -> Result<Self> {
        let mut p = Self::new();
        let mut first = true;
        for raw in text.split('\n') {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if line.is_empty() {
                continue;
            }
            if first {
                first = false; // header row
                continue;
            }
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() < 3 {
                return Err(data_err(format!("Malformed row in portfolio CSV: {line}")));
            }
            let non_numeric = || data_err(format!("Non-numeric quantity/price in row: {line}"));
            let quantity = parse_number(fields[1]).ok_or_else(non_numeric)?;
            let price = parse_number(fields[2]).ok_or_else(non_numeric)?;
            let volatility = match fields.get(3).map(|f| f.trim()) {
                None | Some("") => 0.0,
                Some(v) => match parse_number(v) {
                    Some(x) if x >= 0.0 => x,
                    _ => return Err(data_err(format!("Invalid volatility in row: {line}"))),
                },
            };
            p.add_asset_with_volatility(fields[0].trim(), quantity, price, volatility);
        }
        Ok(p)
    }

    /// Loads a CSV file.
    ///
    /// # Errors
    /// [`Error::Data`] if the file cannot be read or parsed.
    pub fn from_csv_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|_| data_err(format!("Could not open portfolio CSV: {}", path.display())))?;
        Self::from_csv_str(&text)
    }

    /// Checks the portfolio before any simulation work.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an empty portfolio, an empty symbol, a
    /// non-finite quantity or price, or a negative price.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::InvalidArgument(m));
        if self.assets.is_empty() {
            return bad("Portfolio has no assets.".into());
        }
        for a in &self.assets {
            if a.symbol.is_empty() {
                return bad("Asset with empty symbol.".into());
            }
            if a.quantity.is_nan() || a.price.is_nan() {
                return bad(format!("NaN quantity/price for asset: {}", a.symbol));
            }
            if a.quantity.is_infinite() || a.price.is_infinite() {
                return bad(format!("Infinite quantity/price for asset: {}", a.symbol));
            }
            if a.price < 0.0 {
                return bad(format!("Negative price for asset: {}", a.symbol));
            }
            if !(a.volatility.is_finite() && a.volatility >= 0.0) {
                return bad(format!("Invalid volatility for asset: {}", a.symbol));
            }
        }
        Ok(())
    }
}
