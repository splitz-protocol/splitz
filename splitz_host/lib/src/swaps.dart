/// Settling a debt in an asset that is not ZEC (SPEC.md §9.2, `swap`).
///
/// A recipient whose first payout preference is a `swap` cannot be an output
/// of a ZIP 321 request: §8.5 leaves them out and reports them. This is the
/// other half — a deposit address to send ZEC to, and a provider that
/// delivers the asset they asked for on the chain they named.
///
/// **Nothing here names a provider, a host or a URL.** The endpoint, the
/// transport and the referral are the embedding wallet's, injected like the
/// relay's. A wallet that ships its own proxy passes its origin; one that
/// talks to a provider directly passes theirs.
///
/// **A swap is verifiable only in half** (§9.2). What leaves the payer's
/// wallet is ZEC and is recorded in `zatoshi`; what the recipient was owed
/// arrives as another asset on another chain, which the bill cannot see. A
/// caller MUST NOT present a swap as confirmed on the strength of the ZEC leg
/// alone — only the recipient can say they were paid (§10.5).
library;

import 'dart:convert';

import 'package:splitz_core/splitz_core.dart' as protocol;

import 'relay.dart' show JsonGet, JsonPost;

/// A swap that could not be arranged.
class SwapException implements Exception {
  const SwapException(this.message, {this.isTransient = false});

  final String message;

  /// True when retrying the same request could succeed — a timeout, a 5xx.
  /// A quote the provider refused on its merits is not transient.
  final bool isTransient;

  @override
  String toString() => 'SwapException: $message';
}

/// An asset a provider will deliver, named by both halves.
///
/// **Asset and chain are read together, never separately.** One symbol exists
/// on many chains, and a swap matched on the symbol alone delivers the right
/// token to the wrong network, where the recipient cannot reach it.
class TradableAsset {
  const TradableAsset({
    required this.assetId,
    required this.symbol,
    required this.chain,
    required this.decimals,
  });

  /// The provider's own identifier, passed back verbatim.
  final String assetId;

  /// What a person calls it — `USDC`. Not unique across chains.
  final String symbol;

  /// The network it is delivered on — `base`, `arb`. Not unique across
  /// symbols.
  final String chain;

  /// How many base units make one whole token, as a power of ten.
  final int decimals;

  /// Whether this is what [symbol] on [chain] asked for, ignoring case.
  ///
  /// Both halves must match. A payout naming `USDC` on `base` is not
  /// satisfied by `USDC` on any other chain.
  bool answers(String wantedSymbol, String wantedChain) =>
      symbol.toLowerCase() == wantedSymbol.toLowerCase() &&
      chain.toLowerCase() == wantedChain.toLowerCase();
}

/// Where to send ZEC, and what the recipient gets for it.
class SwapQuote {
  const SwapQuote({
    required this.depositAddress,
    required this.amountInZatoshi,
    required this.amountOut,
    required this.asset,
    required this.deadline,
    this.depositMemo,
    this.reference,
    this.minAmountOut,
    this.recipient,
  });

  /// The address the payer's ZEC goes to. **Not the recipient's address** —
  /// the provider's, for this one swap.
  final String depositAddress;

  /// The payout address the provider delivers to: what this quote was taken
  /// for. A deposit is refused once the payee's payout no longer names it —
  /// otherwise the money goes to an address they replaced. Null for a quote
  /// rebuilt to follow a swap already sent.
  final String? recipient;

  /// Some chains need a memo alongside the address; sending without it loses
  /// the deposit.
  final String? depositMemo;

  /// What leaves the payer's wallet. This is the figure a payment record's
  /// `zatoshi` carries (§9.2).
  final int amountInZatoshi;

  /// What the provider quotes the recipient receives, in [asset]'s base
  /// units. Up to the slippage less may arrive; [minAmountOut] is the floor.
  final String amountOut;

  /// The least the recipient receives once slippage is applied, in [asset]'s
  /// base units, or null when the provider states none.
  final String? minAmountOut;

  final TradableAsset asset;

  /// After this the quote is not honoured and a new one is needed.
  ///
  /// A §9.3 instant. Fixed width, so two devices compare it as text and reach
  /// one answer without a calendar between them.
  final String deadline;

  /// The provider's own identifier for this swap.
  ///
  /// This is what a payment record's `reference` carries (§9.2) — **not a
  /// Zcash txid**, and a reader that renders it as one is wrong for every
  /// swap. Absent when the provider names the swap only by its deposit
  /// address, in which case that is the reference.
  final String? reference;

  /// Whether [now], a §9.3 instant, is at or past [deadline].
  bool hasExpired(String now) => now.compareTo(deadline) >= 0;

  /// What a payment record should carry as its `reference` (§9.2).
  String get paymentReference => reference ?? depositAddress;
}

/// Where a swap has got to.
enum SwapState {
  /// The provider has not seen the deposit.
  awaitingDeposit,

  /// The deposit landed; the asset has not been delivered.
  processing,

  /// The swap will not complete and the provider has begun returning the
  /// ZEC to the refund address. Not finished: [failed] follows once it has.
  refunding,

  /// The provider reports the recipient was paid. **Still not a
  /// confirmation**: §10.5 says only the recipient settles a debt.
  delivered,

  /// The swap will not complete. The ZEC may have been refunded.
  failed,
}

/// What a provider says about a swap in flight.
class SwapStatus {
  const SwapStatus({required this.state, this.destinationTxHash, this.detail});

  final SwapState state;

  /// The transaction on the destination chain, when there is one. Named by
  /// the chain of the `swap` payout being settled, never by this one.
  final String? destinationTxHash;

  /// What to put in front of a person. Present on [SwapState.failed].
  final String? detail;
}

/// Arranges swaps off this chain.
///
/// Implemented against a provider by the embedding wallet, or by
/// [OneClickSwaps] for a provider speaking the 1Click shape.
///
/// Specified in SPEC.md §15.7.
abstract interface class SwapProvider {
  /// Every asset this provider will deliver.
  ///
  /// Read before quoting, so a payout naming an asset the provider does not
  /// carry is refused before a person is asked to send anything.
  Future<List<TradableAsset>> tradableAssets();

  /// Quotes sending [amountInZatoshi] of ZEC so that [recipient] is paid in
  /// [asset].
  ///
  /// [refundTo] is where the ZEC goes back to if the swap fails, and is the
  /// payer's own address. A quote with no refund address risks the deposit.
  Future<SwapQuote> quote({
    required TradableAsset asset,
    required int amountInZatoshi,
    required String recipient,
    required String refundTo,
  });

  /// What has happened to the swap [quote] arranged.
  Future<SwapStatus> statusOf(SwapQuote quote);
}

/// A provider that arranges nothing, for a build configured with none.
///
/// Every call fails and says why. The alternative is a swap button that
/// silently does nothing.
class UnconfiguredSwaps implements SwapProvider {
  const UnconfiguredSwaps();

  static const String _why =
      'This build has no swap provider configured, so '
      'a debt owed in another asset cannot be settled here.';

  @override
  Future<List<TradableAsset>> tradableAssets() async => const [];

  @override
  Future<SwapQuote> quote({
    required TradableAsset asset,
    required int amountInZatoshi,
    required String recipient,
    required String refundTo,
  }) async => throw const SwapException(_why);

  @override
  Future<SwapStatus> statusOf(SwapQuote quote) async =>
      throw const SwapException(_why);
}

/// The asset id ZEC is named by when quoting.
///
/// The provider's own identifier for the origin side. Passed in rather than
/// assumed: a provider that lists Zcash under another id would otherwise be
/// quoted for something else entirely.
typedef ZecAssetId = String;

/// A [SwapProvider] speaking the 1Click request shape.
///
/// Three endpoints, relative to [origin]: `GET /v0/tokens`, `POST /v0/quote`,
/// `GET /v0/status`. Held to the provider's schema by
/// `test/oneclick_contract_test.dart`. [origin] is the wallet's —
/// a provider's own host, or a proxy the wallet runs so no credential ships
/// in the app.
///
/// No credential is held here. A deployment needing one puts it behind its
/// own origin, which is why [origin] is required and has no default.
class OneClickSwaps implements SwapProvider {
  OneClickSwaps({
    required this.origin,
    required this.zecAssetId,
    required JsonPost post,
    required JsonGet get,
    required String Function() deadline,
    this.referral,
  }) : _post = post,
       _get = get,
       _deadline = deadline;

  final Uri origin;

  /// How the provider names ZEC. Read from its own token list rather than
  /// guessed; [tradableAssets] is what a caller matches against.
  final ZecAssetId zecAssetId;

  /// Identifies the integrator to the provider, where it asks for one.
  final String? referral;

  /// How long the caller asks a quote to stand for, as a §9.3 instant.
  ///
  /// Supplied rather than computed: a clock and a calendar are the wallet's
  /// (§15.1), and this package carries neither.
  final String Function() _deadline;

  final JsonPost _post;
  final JsonGet _get;

  List<TradableAsset>? _tokens;

  /// How far the delivered amount may fall below the quote, in basis points
  /// (1/100 of a percent). The provider requires it on every quote; 100 is 1%.
  static const int slippageBasisPoints = 100;

  Uri _url(String path, [Map<String, String>? query]) =>
      origin.replace(path: '${origin.path}$path', queryParameters: query);

  @override
  Future<List<TradableAsset>> tradableAssets() async {
    final cached = _tokens;
    if (cached != null) return cached;
    final body = await _read(() => _get(_url('/v0/tokens')), 'tokens');
    final raw = body is List ? body : (body as Map)['tokens'];
    if (raw is! List) {
      throw const SwapException('The provider listed no tokens');
    }
    final tokens = <TradableAsset>[
      for (final entry in raw)
        if (entry is Map<String, dynamic>) _asset(entry),
    ];
    _tokens = tokens;
    return tokens;
  }

  static TradableAsset _asset(Map<String, dynamic> t) => TradableAsset(
    assetId: _string(t, 'assetId'),
    symbol: _string(t, 'symbol'),
    chain: _string(t, 'blockchain'),
    decimals: _decimals(t),
  );

  /// The token's `decimals`, which the schema requires.
  ///
  /// Refused rather than defaulted: a guessed figure shows what arrives off
  /// by a power of ten, beside a deposit that cannot be taken back.
  static int _decimals(Map<String, dynamic> t) {
    final v = t['decimals'];
    if (v is! int || v < 0) {
      throw const SwapException('The provider omitted decimals');
    }
    return v;
  }

  @override
  Future<SwapQuote> quote({
    required TradableAsset asset,
    required int amountInZatoshi,
    required String recipient,
    required String refundTo,
  }) async {
    if (amountInZatoshi <= 0) {
      throw const SwapException('A swap sends more than nothing');
    }
    if (recipient.isEmpty || refundTo.isEmpty) {
      // A quote with no refund address risks the whole deposit if the swap
      // fails, which is the one failure the payer cannot recover from.
      throw const SwapException(
        'A swap states both who receives it and where a refund goes',
      );
    }
    final deadline = _deadline();
    final request = <String, Object?>{
      'dry': false,
      'swapType': 'EXACT_INPUT',
      'slippageTolerance': slippageBasisPoints,
      'originAsset': zecAssetId,
      'depositType': 'ORIGIN_CHAIN',
      'destinationAsset': asset.assetId,
      'amount': '$amountInZatoshi',
      'refundTo': refundTo,
      'refundType': 'ORIGIN_CHAIN',
      'recipient': recipient,
      'recipientType': 'DESTINATION_CHAIN',
      'deadline': deadline,
      'depositMode': 'SIMPLE',
      if (referral != null && referral!.isNotEmpty) 'referral': referral,
    };

    final body = await _read(
      () => _post(_url('/v0/quote'), jsonEncode(request)),
      'quote',
    );
    if (body is! Map<String, dynamic>) {
      throw const SwapException('Malformed quote response');
    }
    final quote = body['quote'];
    if (quote is! Map<String, dynamic>) {
      throw const SwapException('A quote response carries a quote');
    }
    // The answer must be to the question asked. The provider echoes the
    // request it quoted (`quoteRequest`, required by its schema); a quote for
    // another recipient, asset or amount delivers somebody else's money, or
    // this payer's to somebody else, and nothing downstream would notice.
    final echoed = body['quoteRequest'];
    if (echoed is! Map<String, dynamic>) {
      throw const SwapException(
        'A quote response carries the request it quotes',
      );
    }
    for (final field in const [
      'recipient',
      'destinationAsset',
      'originAsset',
      'amount',
      'refundTo',
      'swapType',
    ]) {
      if ('${echoed[field]}' != '${request[field]}') {
        throw SwapException('The provider quoted a different $field');
      }
    }
    if ('${quote['amountIn']}' != '$amountInZatoshi') {
      throw const SwapException('The provider quoted a different amount in');
    }
    return SwapQuote(
      depositAddress: _string(quote, 'depositAddress'),
      depositMemo: _optional(quote, 'depositMemo'),
      amountInZatoshi: amountInZatoshi,
      amountOut: _string(quote, 'amountOut'),
      minAmountOut: _optional(quote, 'minAmountOut'),
      asset: asset,
      // The provider's own deadline where it states one: honouring a longer
      // one of ours would quote a price it has stopped holding.
      deadline: _instant(quote['deadline']) ?? deadline,
      reference: _optional(body, 'correlationId'),
      recipient: recipient,
    );
  }

  @override
  Future<SwapStatus> statusOf(SwapQuote quote) async {
    final body = await _read(
      () => _get(
        _url('/v0/status', {
          'depositAddress': quote.depositAddress,
          if (quote.depositMemo != null && quote.depositMemo!.isNotEmpty)
            'depositMemo': quote.depositMemo!,
        }),
      ),
      'status',
    );
    if (body is! Map<String, dynamic>) {
      throw const SwapException('Malformed status response');
    }
    final status = (body['status'] ?? '').toString().toUpperCase();
    // Everything past the status sits under `swapDetails`: the delivery's
    // hash in `destinationChainTxHashes[].hash`, a failure's reason in
    // `refundReason`.
    final details = body['swapDetails'];
    final detailsMap = details is Map<String, dynamic> ? details : null;
    final hashes = detailsMap?['destinationChainTxHashes'];
    final first = hashes is List && hashes.isNotEmpty ? hashes.first : null;
    // A provider reports a refund under way with the same status word as a
    // delivery under way; only a positive `refundedAmount` tells them apart.
    final state = _state(status);
    final refunding =
        (state == SwapState.awaitingDeposit || state == SwapState.processing) &&
        _positiveDecimal(detailsMap?['refundedAmount']);
    return SwapStatus(
      state: refunding ? SwapState.refunding : state,
      destinationTxHash: first is Map<String, dynamic>
          ? _optional(first, 'hash')
          : null,
      detail: detailsMap == null ? null : _optional(detailsMap, 'refundReason'),
    );
  }

  /// The provider's own vocabulary, mapped onto §9.2's three answers.
  ///
  /// **An unrecognised status is [SwapState.processing], never
  /// [SwapState.delivered].** Reading an unknown word as success would tell a
  /// payer their debt is settled on the strength of a string nobody here has
  /// defined.
  ///
  /// `INCOMPLETE_DEPOSIT` is awaiting a deposit: the provider has received
  /// less than the quote asked for, and nothing has been delivered on it.
  static SwapState _state(String status) => switch (status) {
    'PENDING_DEPOSIT' ||
    'KNOWN_DEPOSIT_TX' ||
    'INCOMPLETE_DEPOSIT' => SwapState.awaitingDeposit,
    'PROCESSING' => SwapState.processing,
    'SUCCESS' => SwapState.delivered,
    'FAILED' || 'REFUNDED' => SwapState.failed,
    _ => SwapState.processing,
  };

  Future<Object?> _read(Future<String> Function() call, String what) async {
    final String text;
    try {
      text = await call();
    } on SwapException {
      rethrow;
    } catch (e) {
      // The transport is the wallet's, so its failures are its own types. A
      // network fault is retryable; nothing here can tell which, so the
      // caller is told it may be.
      throw SwapException(
        'The swap provider could not be reached: $e',
        isTransient: true,
      );
    }
    try {
      return jsonDecode(text);
    } on FormatException {
      throw SwapException('The $what response is not JSON');
    }
  }

  static String _string(Map<String, dynamic> o, String key) {
    final v = o[key];
    if (v is! String || v.isEmpty) {
      throw SwapException('The provider omitted $key');
    }
    return v;
  }

  /// Whether [raw] is a decimal string of digits naming more than zero.
  ///
  /// Read as text rather than as a number: the schema types the amount as a
  /// string, and a figure past 64 bits must not wrap to zero or below.
  static bool _positiveDecimal(Object? raw) =>
      raw is String &&
      RegExp(r'^[0-9]+$').hasMatch(raw) &&
      raw.contains(RegExp('[1-9]'));

  static String? _optional(Map<String, dynamic> o, String key) {
    final v = o[key];
    return (v is String && v.isNotEmpty) ? v : null;
  }

  /// The provider's own deadline, as a §9.3 instant, or null when it states
  /// none this reader can read.
  static String? _instant(Object? raw) {
    if (raw is! String) return null;
    try {
      return protocol.canonicalInstant(raw);
    } on protocol.SplitError {
      return null;
    }
  }
}
