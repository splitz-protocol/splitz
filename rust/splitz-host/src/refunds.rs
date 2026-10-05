//! Whether a refund on the bill accounts for what a payment carries beyond
//! the debts it covers (§6.3).

use std::collections::BTreeSet;

use splitz_core::host::FoldedBill;
use splitz_core::{checked_add, checked_sub, code, split_expense, Settlement};

/// The refunds behind a settlement's unexplained part: how much the bill's
/// negative expenses move onto its payer, and who wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundsBehind {
    /// What the refunds move onto the payer, in the bill's minor units: at
    /// least the settlement's unexplained part.
    pub refunded: i64,
    /// Who wrote them, sorted; an expense with no known author is the empty
    /// string, so it never reads as the payer's own doing.
    pub authors: Vec<String>,
}

/// The refunds that account for `settlement`'s unexplained part, or `None`
/// when none do, or it has none (§6.3).
///
/// A refund is an expense below zero the payer is down as having paid: the
/// shares it gives everybody else move onto the payer. Only when those cover
/// the whole unexplained part may a host call it a refund; otherwise the bill
/// holds no refund that explains it — a confirmed payment above what was owed
/// leaves the same figure — and a host says only that no debt explains it.
pub fn refunds_behind(settlement: &Settlement, folded: &FoldedBill) -> Option<RefundsBehind> {
    let unexplained = settlement.unexplained();
    if unexplained <= 0 {
        return None;
    }
    let mut refunded = 0i64;
    let mut authors: BTreeSet<String> = BTreeSet::new();
    for e in &folded.bill.expenses {
        if e.amount >= 0 || e.paid_by != settlement.from {
            continue;
        }
        let mut moved = 0i64;
        for (id, share) in split_expense(e.amount, &e.split).ok()? {
            if id != settlement.from {
                moved = checked_sub(moved, share, code::AMOUNT_OVERFLOW).ok()?;
            }
        }
        if moved <= 0 {
            continue;
        }
        refunded = checked_add(refunded, moved, code::AMOUNT_OVERFLOW).ok()?;
        authors.insert(
            folded
                .expense_authors
                .get(&e.id)
                .cloned()
                .unwrap_or_default(),
        );
    }
    if refunded < unexplained {
        return None;
    }
    // Sorted by UTF-8 bytes, as §2.3 orders ids, which a BTreeSet of String is.
    Some(RefundsBehind {
        refunded,
        authors: authors.into_iter().collect(),
    })
}
