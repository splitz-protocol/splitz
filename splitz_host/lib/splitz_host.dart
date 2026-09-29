/// Split bills in a Zcash wallet — everything except the screens.
///
/// `package:splitz` decides what a bill is, what anyone owes and which payment
/// request settles it. This package is what a wallet needs around that: the
/// seam it plugs into, entry signing, sealing, the log it keeps, and the sync
/// that moves a bill between devices.
///
/// **It names no wallet and depends on none.** Everything a wallet supplies —
/// which account speaks, how a payment request is sent, where a secret lives,
/// the clock, the randomness, the price of a ZEC, the network route — is
/// declared here as an interface and injected. Nothing under `lib/` imports
/// outside this package, and there is nothing for it to import: a feature
/// written inside one wallet reaches for whatever sits next to it and then
/// cannot be lifted into another.
///
/// Screens are somebody else's. They are where a wallet's own shape belongs,
/// and two wallets should not have to agree about them to agree about money.
library;

export 'src/currencies.dart';
export 'src/fold.dart';
export 'src/keys.dart';
export 'src/pending_sends.dart';
export 'src/pricing.dart';
export 'src/activity.dart';
export 'src/relay.dart';
export 'src/split_draft.dart';
export 'src/swap_watch.dart';
export 'src/swaps.dart';
export 'src/sealing.dart';
export 'src/signing.dart';
export 'src/store.dart';
export 'src/sync.dart';
export 'src/testing/payer_review.dart';
export 'src/testing/seam_contracts.dart';
export 'src/testing/seed_driver.dart';
export 'src/wallet.dart';
export 'src/wallet_bill_host.dart';
