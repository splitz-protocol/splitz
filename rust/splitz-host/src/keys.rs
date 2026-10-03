//! Where a bill's key and this account's signing identity live.

use splitz_core::sha256;

use crate::error::{HostError, Result};
use crate::signing::{base64url_decode, base64url_encode};
use crate::wallet::{SecretStore, WalletAccount};

const BILL_PREFIX: &str = "splitz_bill_key_";
// v2: an identity stored under the v1 name was derived from a viewing key,
// which a wallet hands out, so it is never read again.
const IDENTITY_PREFIX: &str = "splitz_identity_seed_v2_";

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
pub const IDENTITY_DOMAIN: &str = "splitz.identity.v2";

/// Bytes nobody can predict.
///
/// §9.4 derives a bill's id from a nonce, so two bills created in the same
/// second by the same person are the same bill unless this is unpredictable.
/// The wallet supplies it because the wallet knows what secure randomness
/// means on its platform.
pub trait Randomness {
    fn bytes(&self, count: usize) -> Vec<u8>;
}

/// The platform's own entropy.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRandomness;

impl Randomness for SystemRandomness {
    fn bytes(&self, count: usize) -> Vec<u8> {
        let mut out = vec![0u8; count];
        getrandom::fill(&mut out).expect("the platform has no entropy source");
        out
    }
}

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
    ///
    /// Refuses, with [`HostError::KeyConflict`], a different key for a bill
    /// this device already holds a key for. Replacing it would seal everything
    /// this device writes under a key the others do not hold, and open nothing
    /// they write — and an invite link is text anyone can send. The same key
    /// again is a no-op.
    pub fn store_bill_key(&self, bill_id: &str, key: &str) -> Result<()> {
        if !is_well_formed_key(key) {
            return Err(HostError::Malformed(format!(
                "a bill key is {KEY_LENGTH_BYTES} bytes of base64url"
            )));
        }
        match self.read_bill_key(bill_id)? {
            Some(held) if !held.is_empty() && held == key => Ok(()),
            Some(held) if !held.is_empty() => Err(HostError::KeyConflict(bill_id.to_owned())),
            _ => self.store.write(&Self::bill_key_name(bill_id), key),
        }
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
    /// [`WalletAccount::identity_secret`] is what makes it survive a
    /// reinstall: the seed is [`identity_seed_from`] the secret. An account
    /// with no secret gets a random seed: it signs correctly and simply cannot
    /// be recovered, which is a state to report rather than one to paper over.
    pub fn ensure_identity_seed(&self, account: &WalletAccount) -> Result<Vec<u8>> {
        let name = Self::identity_name(&account.id);
        if let Some(stored) = self.store.read(&name)? {
            if !stored.is_empty() {
                return base64url_decode(&stored).ok_or_else(|| {
                    HostError::Malformed("a stored identity seed is base64url".to_owned())
                });
            }
        }
        let seed = match account.identity_secret.as_deref() {
            Some(secret) if !secret.is_empty() => identity_seed_from(secret),
            _ => self.random.bytes(KEY_LENGTH_BYTES),
        };
        self.store.write(&name, &base64url_encode(&seed))?;
        Ok(seed)
    }
}

/// The identity seed `secret` derives: SHA-256 of [`IDENTITY_DOMAIN`], a zero
/// byte, and the secret.
pub fn identity_seed_from(secret: &[u8]) -> Vec<u8> {
    let mut message = IDENTITY_DOMAIN.as_bytes().to_vec();
    message.push(0);
    message.extend_from_slice(secret);
    sha256(&message).to_vec()
}

/// The highest ZIP 32 account index: account indices are below 2^31.
pub const MAX_ACCOUNT_INDEX: u32 = 0x7fff_ffff;

/// The identity secret of a BIP39 wallet account (§15.1): the mnemonic and
/// the passphrase, UTF-8, joined by a zero byte, and for any ZIP 32 account
/// but account 0, a zero byte and `account_index` as four big-endian bytes.
///
/// The passphrase is part of it because it selects another wallet from one
/// mnemonic; the account index because two accounts of one mnemonic are two
/// people to a bill. Neither text may hold a zero byte, so no two inputs join
/// to the same bytes. Every wallet derives the same bytes, so one person
/// is one participant whichever wallet they restore into.
///
/// Refuses an empty mnemonic, whose identity anyone could derive, either text
/// holding a zero byte, and an account index above [`MAX_ACCOUNT_INDEX`].
pub fn identity_secret_from_mnemonic(
    mnemonic: &str,
    passphrase: &str,
    account_index: u32,
) -> Result<Vec<u8>> {
    if mnemonic.is_empty() {
        return Err(HostError::Malformed(
            "an empty mnemonic derives an identity anyone can compute".to_owned(),
        ));
    }
    // The zero byte is the separator: one inside either text would let two
    // different inputs join to one secret, and so one identity.
    if mnemonic.contains('\0') || passphrase.contains('\0') {
        return Err(HostError::Malformed(
            "a mnemonic or passphrase holding a zero byte is not one a wallet derives".to_owned(),
        ));
    }
    if account_index > MAX_ACCOUNT_INDEX {
        return Err(HostError::Malformed(format!(
            "account index {account_index} is above the ZIP 32 maximum {MAX_ACCOUNT_INDEX}"
        )));
    }
    let mut secret = mnemonic.as_bytes().to_vec();
    secret.push(0);
    secret.extend_from_slice(passphrase.as_bytes());
    if account_index != 0 {
        secret.push(0);
        secret.extend_from_slice(&account_index.to_be_bytes());
    }
    Ok(secret)
}

/// Whether `key` is one the cipher can actually use: base64url, padded or not,
/// decoding to exactly [`KEY_LENGTH_BYTES`] bytes.
pub fn is_well_formed_key(key: &str) -> bool {
    !key.is_empty() && base64url_decode(key).is_some_and(|b| b.len() == KEY_LENGTH_BYTES)
}
