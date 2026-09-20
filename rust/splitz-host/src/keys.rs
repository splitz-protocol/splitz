//! Where a bill's key and this account's signing identity live.

use splitz_core::sha256;

use crate::error::{HostError, Result};
use crate::signing::{base64url_decode, base64url_encode};
use crate::wallet::{Randomness, SecretStore, WalletAccount};

const BILL_PREFIX: &str = "splitz_bill_key_";
const IDENTITY_PREFIX: &str = "splitz_identity_seed_";

/// Key length in bytes. 32 suits XChaCha20 and AES-256 alike, so the choice of
/// cipher stays open, and it is also Ed25519's seed length.
pub const KEY_LENGTH_BYTES: usize = 32;

/// Domain separator for the derived identity seed.
///
/// Names the protocol, not a wallet. A wallet-specific domain would give one
/// person two identities across two wallets, and a bill would stop recognising
/// them the day they switched — the opposite of what a protocol several
/// wallets implement is for.
///
/// Changing it changes every identity derived afterwards, so it is versioned
/// rather than edited.
pub const IDENTITY_DOMAIN: &str = "splitz.identity.v1";

/// Holds the symmetric key each bill's contents are sealed under, and the
/// Ed25519 seed this account signs with.
///
/// Both are secrets and both live in the wallet's [`SecretStore`]. Neither is
/// derived from anything an invite hands out: a bill key derived from the bill
/// id would be reproducible by everyone who ever saw a scanned code, which is
/// the one thing an invite gives away freely.
pub struct SplitsKeys<'a> {
    store: &'a dyn SecretStore,
    random: &'a dyn Randomness,
}

impl<'a> SplitsKeys<'a> {
    pub fn new(store: &'a dyn SecretStore, random: &'a dyn Randomness) -> Self {
        Self { store, random }
    }

    fn bill_key_name(bill_id: &str) -> String {
        format!("{BILL_PREFIX}{bill_id}")
    }

    fn identity_name(account_id: &str) -> String {
        format!("{IDENTITY_PREFIX}{account_id}")
    }

    /// The bill's key, creating and storing one on first use.
    pub fn ensure_bill_key(&self, bill_id: &str) -> Result<String> {
        if let Some(existing) = self.read_bill_key(bill_id)? {
            if !existing.is_empty() {
                return Ok(existing);
            }
        }
        let key = self.generate_key();
        self.store.write(&Self::bill_key_name(bill_id), &key)?;
        Ok(key)
    }

    /// The stored key, or `None` when this device holds none for that bill.
    pub fn read_bill_key(&self, bill_id: &str) -> Result<Option<String>> {
        self.store.read(&Self::bill_key_name(bill_id))
    }

    /// Stores a key that arrived with an invite, so a joiner can open the bill.
    ///
    /// Refuses a key the cipher cannot use. §11.1 checks only that an invite's
    /// `k` is non-empty base64url — not that it is the right length — so
    /// without this a malformed key is stored, and the failure then surfaces
    /// from inside whatever loop next tries to decrypt, far from the scan that
    /// caused it.
    pub fn store_bill_key(&self, bill_id: &str, key: &str) -> Result<()> {
        if !is_well_formed_key(key) {
            return Err(HostError::Malformed(format!(
                "a bill key is {KEY_LENGTH_BYTES} bytes of base64url"
            )));
        }
        self.store.write(&Self::bill_key_name(bill_id), key)
    }

    /// Forgets a bill's key, so the keychain does not accumulate secrets for
    /// bills that no longer exist.
    pub fn forget_bill(&self, bill_id: &str) -> Result<()> {
        self.store.delete(&Self::bill_key_name(bill_id))
    }

    /// A fresh key, base64url without padding, as an invite carries one.
    pub fn generate_key(&self) -> String {
        base64url_encode(&self.random.bytes(KEY_LENGTH_BYTES))
    }

    /// The 32-byte Ed25519 seed that is this account's signing identity,
    /// creating and storing one on first use.
    ///
    /// One identity per wallet account, not one per bill: the same key signs
    /// this account's entries on every bill, and its public half — published
    /// in that account's own join — is what other participants pin under
    /// §10.7.
    ///
    /// [`WalletAccount::viewing_key`] is what makes it survive a reinstall.
    /// An account with no viewing key gets a random seed: it signs correctly
    /// and simply cannot be recovered, which is a state to report rather than
    /// one to paper over.
    pub fn ensure_identity_seed(&self, account: &WalletAccount) -> Result<Vec<u8>> {
        let name = Self::identity_name(&account.id);
        if let Some(stored) = self.store.read(&name)? {
            if !stored.is_empty() {
                return base64url_decode(&stored).ok_or_else(|| {
                    HostError::Malformed("a stored identity seed is base64url".to_owned())
                });
            }
        }
        let seed = match account.viewing_key.as_deref() {
            Some(viewing_key) if !viewing_key.is_empty() => {
                sha256(format!("{IDENTITY_DOMAIN}:{viewing_key}").as_bytes()).to_vec()
            }
            _ => self.random.bytes(KEY_LENGTH_BYTES),
        };
        self.store.write(&name, &base64url_encode(&seed))?;
        Ok(seed)
    }
}

/// Whether `key` is one the cipher can actually use: base64url, padded or not,
/// decoding to exactly [`KEY_LENGTH_BYTES`] bytes.
pub fn is_well_formed_key(key: &str) -> bool {
    !key.is_empty() && base64url_decode(key).is_some_and(|b| b.len() == KEY_LENGTH_BYTES)
}
