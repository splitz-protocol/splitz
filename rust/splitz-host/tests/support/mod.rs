//! A wallet that does nothing, for tests that are not about the wallet.
//!
//! Each test binary compiles this file separately and uses a different part
//! of it, so what one leaves unused is not dead.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};

use splitz_host::{
    InMemorySecretStore, SecretStore, SplitsWallet, WalletAccount, WalletSendOutcome,
    WalletSendPhase, WalletSender,
};

/// The clock is held still and the randomness is a counter: §9.3 instants
/// order a log and §9.4 derives a bill id from a nonce, so a log that moved
/// between runs could not be asserted against a fixed expectation.
pub struct FakeWallet {
    pub account: WalletAccount,
    pub sender: FakeSender,
    secrets: InMemorySecretStore,
    minute: Cell<u32>,
    counter: Cell<u8>,
}

impl FakeWallet {
    pub fn new(id: &str, pay_to: Option<&str>) -> Self {
        Self {
            account: WalletAccount {
                id: id.to_owned(),
                identity_secret: Some(format!("secret-{id}").into_bytes()),
            },
            sender: FakeSender::new(pay_to),
            secrets: InMemorySecretStore::default(),
            minute: Cell::new(0),
            counter: Cell::new(0),
        }
    }

    pub fn ana() -> Self {
        Self::new("ana", Some("u1ana000000000000000000"))
    }

    /// Moves the clock on, so two entries written in one test are two entries
    /// and §10.2 has an order to put them in.
    pub fn tick(&self) {
        self.minute.set(self.minute.get() + 1);
    }
}

impl SplitsWallet for FakeWallet {
    fn account(&self) -> &WalletAccount {
        &self.account
    }

    fn sender(&self) -> &dyn WalletSender {
        &self.sender
    }

    fn secrets(&self) -> &dyn SecretStore {
        &self.secrets
    }

    fn now(&self) -> String {
        let total = 19 * 60 + 30 + self.minute.get();
        format!("2026-10-28T{:02}:{:02}:00Z", total / 60, total % 60)
    }

    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.counter.set(self.counter.get().wrapping_add(1));
        (0..byte_count)
            .map(|i| self.counter.get().wrapping_add(i as u8))
            .collect()
    }
}

pub struct FakeSender {
    pay_to: Option<String>,
    pub outcome: WalletSendOutcome,
    pub sent: RefCell<Vec<String>>,
}

impl FakeSender {
    fn new(pay_to: Option<&str>) -> Self {
        Self {
            pay_to: pay_to.map(str::to_owned),
            outcome: WalletSendOutcome {
                phase: WalletSendPhase::Succeeded,
                txid: Some("tx-1".to_owned()),
                status_message: None,
                error: None,
            },
            sent: RefCell::new(Vec::new()),
        }
    }
}

impl WalletSender for FakeSender {
    fn send(&self, payment_request_uri: &str) -> WalletSendOutcome {
        self.sent.borrow_mut().push(payment_request_uri.to_owned());
        self.outcome.clone()
    }

    fn pay_to_address(&self) -> Option<String> {
        self.pay_to.clone()
    }
}

/// A distinct 32-byte Ed25519 seed per person. Fixed, so a failing run can be
/// repeated: §9.4's nonce is what separates two bills, not this.
pub fn seed_for(who: &str) -> Vec<u8> {
    let first = who.as_bytes()[0];
    (0..32).map(|i| first.wrapping_add(i as u8)).collect()
}
