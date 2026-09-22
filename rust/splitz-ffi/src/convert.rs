//! The library's own shapes, as the records a foreign caller reads.
//!
//! One direction only. A caller builds an entry by naming what it wants — an
//! amount, a payee, a split — rather than by assembling a §9 object, so
//! nothing here reads a record back into the library's types.

use crate::records as ffi;

pub(crate) fn payout(p: &splitz_core::Payout) -> ffi::Payout {
    ffi::Payout {
        kind: p.kind.clone(),
        address: p.address.clone(),
        asset: p.asset.clone(),
        chain: p.chain.clone(),
    }
}

pub(crate) fn participant(p: &splitz_core::Participant) -> ffi::Participant {
    ffi::Participant {
        id: p.id.clone(),
        name: p.name.clone(),
        pay_to: p.pay_to.clone(),
        identity_key: p.identity_key.clone(),
        payouts: p.payouts.iter().map(payout).collect(),
    }
}

pub(crate) fn expense(e: &splitz_core::Expense) -> ffi::Expense {
    ffi::Expense {
        id: e.id.clone(),
        description: e.description.clone(),
        paid_by: e.paid_by.clone(),
        amount: e.amount,
        currency: e.currency.clone(),
        at: e.at.clone(),
        split_json: e.split.to_string(),
    }
}

pub(crate) fn rate(r: &splitz_core::rate::ExchangeRate) -> ffi::ExchangeRate {
    ffi::ExchangeRate {
        currency: r.currency.clone(),
        minor_units_per_zec: r.minor_units_per_zec,
        at: r.at.clone(),
        source: r.source.clone(),
    }
}

pub(crate) fn payment(p: &splitz_core::PaymentRecord) -> ffi::PaymentRecord {
    ffi::PaymentRecord {
        id: p.id.clone(),
        from: p.from.clone(),
        to: p.to.clone(),
        amount: p.amount,
        currency: p.currency.clone(),
        method: p.method.clone(),
        at: p.at.clone(),
        zatoshi: p.zatoshi,
        reference: p.reference.clone(),
        note: p.note.clone(),
    }
}

pub(crate) fn bill(b: &splitz_core::Bill) -> ffi::Bill {
    ffi::Bill {
        id: b.id.clone(),
        name: b.name.clone(),
        currency: b.currency.clone(),
        split_mode: b.split_mode.clone(),
        participants: b.participants.iter().map(participant).collect(),
        expenses: b.expenses.iter().map(expense).collect(),
        payments: b.payments.iter().map(payment).collect(),
        confirmed_payments: b.confirmed_payments.iter().cloned().collect(),
        rate: b.rate.as_ref().map(rate),
    }
}

pub(crate) fn set_aside(s: &splitz_core::SetAside) -> ffi::SetAside {
    ffi::SetAside {
        id: s.id.clone(),
        code: s.code.to_owned(),
    }
}

pub(crate) fn folded(f: &splitz_core::host::FoldedBill) -> ffi::FoldedBill {
    ffi::FoldedBill {
        bill: bill(&f.bill),
        set_aside: f.set_aside.iter().map(set_aside).collect(),
        withdrawn: f.withdrawn.clone(),
        replaced_addresses: f
            .replaced_addresses
            .iter()
            .map(|r| ffi::ReplacedAddress {
                id: r.id.clone(),
                from: r.from.clone(),
                to: r.to.clone(),
            })
            .collect(),
        identities: ffi::Identities {
            bound: f.identities.bound.clone().into_iter().collect(),
            contested: f.identities.contested.iter().cloned().collect(),
        },
    }
}

pub(crate) fn settlement(s: &splitz_core::Settlement) -> ffi::Settlement {
    ffi::Settlement {
        from: s.from.clone(),
        to: s.to.clone(),
        amount: s.amount,
        covers: s
            .covers
            .iter()
            .map(|d| ffi::DirectDebt {
                from: d.from.clone(),
                to: d.to.clone(),
                amount: d.amount,
            })
            .collect(),
    }
}

pub(crate) fn obligation(o: &splitz_core::host::PayerObligation) -> ffi::PayerObligation {
    ffi::PayerObligation {
        settlements: o.settlements.iter().map(settlement).collect(),
        awaiting: o
            .awaiting
            .iter()
            .map(|a| ffi::Awaiting {
                to: a.to.clone(),
                owed: a.owed,
                paid: a.paid,
            })
            .collect(),
        contested: o
            .contested
            .iter()
            .map(|c| ffi::Contested {
                to: c.to.clone(),
                amount: c.amount,
                address: c.address.clone(),
            })
            .collect(),
        request: ffi::Obligation {
            uri: o.request.uri.clone(),
            unpayable: o
                .request
                .unpayable
                .iter()
                .map(|u| ffi::Unpayable {
                    id: u.id.clone(),
                    reason: u.reason.to_owned(),
                    minor_units: u.minor_units,
                })
                .collect(),
            carried_minor_units: o.request.carried_minor_units,
            withheld_minor_units: o.request.withheld_minor_units,
        },
    }
}

pub(crate) fn event(e: &splitz_host::BillEvent) -> ffi::BillEvent {
    use splitz_host::BillEventKind as K;
    ffi::BillEvent {
        entry_id: e.entry_id.clone(),
        kind: match e.kind {
            K::Opened => ffi::BillEventKind::Opened,
            K::Joined => ffi::BillEventKind::Joined,
            K::AddressChanged => ffi::BillEventKind::AddressChanged,
            K::ExpenseAdded => ffi::BillEventKind::ExpenseAdded,
            K::ExpenseAmended => ffi::BillEventKind::ExpenseAmended,
            K::EntryWithdrawn => ffi::BillEventKind::EntryWithdrawn,
            K::PaymentRecorded => ffi::BillEventKind::PaymentRecorded,
            K::PaymentConfirmed => ffi::BillEventKind::PaymentConfirmed,
            K::Priced => ffi::BillEventKind::Priced,
            K::Other => ffi::BillEventKind::Other,
        },
        author: e.author.clone(),
        at: e.at.clone(),
        subject: e.subject.clone(),
        amount_minor_units: e.amount_minor_units,
        description: e.description.clone(),
        method: e.method.clone(),
        reference: e.reference.clone(),
        withdrawn: e.withdrawn,
        refused_code: e.refused_code.clone(),
        confirmed: e.confirmed,
        applied: e.applied(),
    }
}

pub(crate) fn asset(a: &splitz_host::TradableAsset) -> ffi::TradableAsset {
    ffi::TradableAsset {
        asset_id: a.asset_id.clone(),
        symbol: a.symbol.clone(),
        chain: a.chain.clone(),
        decimals: a.decimals,
    }
}

pub(crate) fn quote(q: &splitz_host::SwapQuote) -> ffi::SwapQuote {
    ffi::SwapQuote {
        deposit_address: q.deposit_address.clone(),
        deposit_memo: q.deposit_memo.clone(),
        amount_in_zatoshi: q.amount_in_zatoshi,
        amount_out: q.amount_out.clone(),
        min_amount_out: q.min_amount_out.clone(),
        asset: asset(&q.asset),
        deadline: q.deadline.clone(),
        reference: q.reference.clone(),
    }
}

pub(crate) fn asset_back(a: &ffi::TradableAsset) -> splitz_host::TradableAsset {
    splitz_host::TradableAsset {
        asset_id: a.asset_id.clone(),
        symbol: a.symbol.clone(),
        chain: a.chain.clone(),
        decimals: a.decimals,
    }
}

pub(crate) fn status(s: &splitz_host::SwapStatus) -> ffi::SwapStatus {
    use splitz_host::SwapState as S;
    ffi::SwapStatus {
        state: match s.state {
            S::AwaitingDeposit => ffi::SwapState::AwaitingDeposit,
            S::Processing => ffi::SwapState::Processing,
            S::Delivered => ffi::SwapState::Delivered,
            S::Failed => ffi::SwapState::Failed,
        },
        destination_tx_hash: s.destination_tx_hash.clone(),
        detail: s.detail.clone(),
    }
}
