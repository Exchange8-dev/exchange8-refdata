// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Ex8:RefData core — corporate-action adjustments.
//!
//! Corporate-action lifecycle and the ex-date adjustment math (price/qty
//! transformations applied to positions and market data). The shell wires the
//! lifecycle events and Settlement delivery.
//!
//! Design: `design/sentin8-exchange8-corporate-actions-technical-design.md` §2.

/// A corporate-action type.
///
/// `Merger`/`Delisting`/`SymbolChange`/`Rights` are constructed by the
/// shell's announcement ingest (external feed rows); the pure tests cover
/// the adjustment math paths.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionType {
    /// Cash payment to holders on record date; price adjustment on ex-date.
    CashDividend,
    /// Additional shares; price/quantity adjustment.
    StockDividend,
    /// Quantity × ratio, price ÷ ratio.
    Split,
    /// Reverse split: quantity ÷ ratio, price × ratio.
    ReverseSplit,
    /// Security replaced or delisted.
    Merger,
    /// New security issued to holders.
    SpinOff,
    /// Entitlement to subscribe.
    Rights,
    /// Reference-data update + market-data re-key.
    SymbolChange,
    /// Trading halt → removal.
    Delisting,
}

impl ActionType {
    /// Wire key (used in `refdata.corporate_action.type` rows + events).
    pub fn key(self) -> &'static str {
        match self {
            ActionType::CashDividend => "cash_dividend",
            ActionType::StockDividend => "stock_dividend",
            ActionType::Split => "split",
            ActionType::ReverseSplit => "reverse_split",
            ActionType::Merger => "merger",
            ActionType::SpinOff => "spin_off",
            ActionType::Rights => "rights",
            ActionType::SymbolChange => "symbol_change",
            ActionType::Delisting => "delisting",
        }
    }
}

/// Corporate-action lifecycle stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Announced,
    ExDate,
    RecordDate,
    PayDate,
}

impl Stage {
    /// Advance to the next stage; None after PayDate (terminal).
    pub fn next(self) -> Option<Stage> {
        match self {
            Stage::Announced => Some(Stage::ExDate),
            Stage::ExDate => Some(Stage::RecordDate),
            Stage::RecordDate => Some(Stage::PayDate),
            Stage::PayDate => None,
        }
    }
}

/// A corporate action announcement (one instrument).
#[derive(Clone, Debug, PartialEq)]
pub struct CorporateAction {
    pub instrument_id: u32,
    pub action_type: ActionType,
    /// Ratio numerator/denominator (e.g. 2:1 split = (2, 1)).
    /// For dividends: cash amount per share in raw ticks (numerator only).
    pub numerator: i128,
    pub denominator: i128,
    pub stage: Stage,
}

impl CorporateAction {
    /// Adjust an open position's quantity for the ex-date.
    ///
    /// - Split: ratio is old→new multiplier (2:1 → ×2 shares).
    /// - Reverse split: ratio is new:old (1:10 → 10 old shares consolidate to 1
    ///   new, so qty × numerator/denominator).
    /// - Stock dividend: qty × (1 + numerator/denominator).
    /// - Cash dividend / others: qty unchanged (cash is delivered on pay date).
    pub fn adjust_qty(&self, qty: i128) -> i128 {
        match self.action_type {
            ActionType::Split => qty * self.numerator / self.denominator,
            ActionType::ReverseSplit => qty * self.numerator / self.denominator,
            ActionType::StockDividend => qty + qty * self.numerator / self.denominator,
            _ => qty,
        }
    }

    /// Adjust a reference price for the ex-date (the price the market data
    /// re-keys against).
    ///
    /// - Split: price ÷ ratio (2:1 → ÷2).
    /// - Reverse split: price ÷ ratio (1:10 reverse → ×10).
    /// - Cash dividend: price − dividend per share.
    /// - Stock dividend: price ÷ (1 + ratio).
    pub fn adjust_price(&self, price: i128) -> i128 {
        match self.action_type {
            ActionType::Split => price * self.denominator / self.numerator,
            ActionType::ReverseSplit => price * self.denominator / self.numerator,
            ActionType::CashDividend => price - self.numerator,
            ActionType::StockDividend => {
                if self.numerator == 0 {
                    price
                } else {
                    price * self.denominator / (self.denominator + self.numerator)
                }
            }
            _ => price,
        }
    }

    /// Cash entitlement per share on the pay date (raw ticks); 0 for stock
    /// actions.
    pub fn cash_per_share(&self) -> i128 {
        match self.action_type {
            ActionType::CashDividend => self.numerator,
            _ => 0,
        }
    }

    /// Entitlement snapshot on record date: total cash for a holding.
    pub fn cash_entitlement(&self, qty: i128) -> i128 {
        self.cash_per_share() * qty
    }

    /// Stock entitlement on record date: additional shares for a holding.
    pub fn stock_entitlement(&self, qty: i128) -> i128 {
        match self.action_type {
            ActionType::StockDividend | ActionType::SpinOff | ActionType::Rights => {
                qty * self.numerator / self.denominator
            }
            _ => 0,
        }
    }

    /// Advance the lifecycle; irreversible actions (delisting, merger) skip
    /// record/pay and terminate at ex-date + removal.
    pub fn advance(&mut self) -> Option<Stage> {
        let next = self.stage.next()?;
        self.stage = next;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(n: i128, d: i128) -> CorporateAction {
        CorporateAction {
            instrument_id: 1,
            action_type: ActionType::Split,
            numerator: n,
            denominator: d,
            stage: Stage::Announced,
        }
    }

    #[test]
    fn action_type_keys() {
        assert_eq!(ActionType::CashDividend.key(), "cash_dividend");
        assert_eq!(ActionType::ReverseSplit.key(), "reverse_split");
        assert_eq!(ActionType::SymbolChange.key(), "symbol_change");
        assert_eq!(ActionType::Delisting.key(), "delisting");
    }

    #[test]
    fn split_adjusts_qty_and_price() {
        // 2:1 split: 100 shares @ 10.00 → 200 shares @ 5.00.
        let a = split(2, 1);
        assert_eq!(a.adjust_qty(100), 200);
        assert_eq!(a.adjust_price(100_000), 50_000);
    }

    #[test]
    fn reverse_split_adjusts() {
        // 1:10 reverse split: 1000 shares @ 1.00 → 100 shares @ 10.00.
        let mut a = split(1, 10);
        a.action_type = ActionType::ReverseSplit;
        assert_eq!(a.adjust_qty(1_000), 100);
        assert_eq!(a.adjust_price(10_000), 100_000);
    }

    #[test]
    fn cash_dividend_adjusts_price_not_qty() {
        let mut a = split(25_000, 0); // 2.50 raw ticks per share
        a.action_type = ActionType::CashDividend;
        assert_eq!(a.adjust_qty(100), 100); // qty unchanged
        assert_eq!(a.adjust_price(100_000), 75_000); // 10.00 − 2.50
        assert_eq!(a.cash_per_share(), 25_000);
        assert_eq!(a.cash_entitlement(100), 2_500_000); // 100 × 2.50
    }

    #[test]
    fn stock_dividend_adjusts() {
        // 10% stock dividend: 100 shares @ 10.00 → 110 shares, price ÷ 1.1.
        let mut a = split(1, 10);
        a.action_type = ActionType::StockDividend;
        assert_eq!(a.adjust_qty(100), 110);
        assert_eq!(a.adjust_price(100_000), 100_000 * 10 / 11);
        assert_eq!(a.stock_entitlement(100), 10);
        assert_eq!(a.cash_entitlement(100), 0);
    }

    #[test]
    fn lifecycle_advances_to_pay_date() {
        let mut a = split(2, 1);
        assert_eq!(a.stage, Stage::Announced);
        assert_eq!(a.advance(), Some(Stage::ExDate));
        assert_eq!(a.advance(), Some(Stage::RecordDate));
        assert_eq!(a.advance(), Some(Stage::PayDate));
        assert_eq!(a.advance(), None); // terminal
        assert_eq!(a.stage, Stage::PayDate);
    }

    #[test]
    fn spin_off_entitlements() {
        let mut a = split(1, 4); // 1 new share per 4 held
        a.action_type = ActionType::SpinOff;
        assert_eq!(a.stock_entitlement(400), 100);
        assert_eq!(a.cash_entitlement(400), 0);
        // Price unchanged (the new security is separate).
        assert_eq!(a.adjust_price(100_000), 100_000);
    }

    #[test]
    fn rights_issue_entitlement() {
        let mut a = split(1, 5); // 1 right per 5 held
        a.action_type = ActionType::Rights;
        assert_eq!(a.stock_entitlement(500), 100);
    }
}
