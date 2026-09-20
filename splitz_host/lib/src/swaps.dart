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
  });

  /// The address the payer's ZEC goes to. **Not the recipient's address** —
  /// the provider's, for this one swap.
  final String depositAddress;

  /// Some chains need a memo alongside the address; sending without it loses
  /// the deposit.
  final String? depositMemo;

  /// What leaves the payer's wallet. This is the figure a payment record's
  /// `zatoshi` carries (§9.2).
  final int amountInZatoshi;

  /// What the recipient receives, in [asset]'s base units.
  final String amountOut;

  final TradableAsset asset;

  /// After this the quote is not honoured and a new one is needed.
  final DateTime deadline;

  /// The provider's own identifier for this swap.
  ///
  /// This is what a payment record's `reference` carries (§9.2) — **not a
  /// Zcash txid**, and a reader that renders it as one is wrong for every
  /// swap. Absent when the provider names the swap only by its deposit
  /// address, in which case that is the reference.
  final String? reference;

  /// Whether [now] is past [deadline].
  bool hasExpired(DateTime now) => !now.isBefore(deadline);

  /// What a payment record should carry as its `reference` (§9.2).
  String get paymentReference => reference ?? depositAddress;
}

/// Where a swap has got to.
enum SwapState {
  /// The provider has not seen the deposit.
  awaitingDeposit,

  /// The deposit landed; the asset has not been delivered.
  processing,

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
/// Four endpoints, relative to [origin]: `GET /v0/tokens`, `POST /v0/quote`,
/// `GET /v0/status`, `POST /v0/deposit/submit`. [origin] is the wallet's —
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
    this.referral,
    this.quoteValidity = const Duration(minutes: 10),
    DateTime Function()? now,
  }) : _post = post,
       _get = get,
       _now = now ?? DateTime.now;

  final Uri origin;

  /// How the provider names ZEC. Read from its own token list rather than
  /// guessed; [tradableAssets] is what a caller matches against.
  final ZecAssetId zecAssetId;

  /// Identifies the integrator to the provider, where it asks for one.
  final String? referral;

  /// How long a quote is asked to stand for.
  final Duration quoteValidity;

  final JsonPost _post;
  final JsonGet _get;
  final DateTime Function() _now;

  List<TradableAsset>? _tokens;

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
    decimals: t['decimals'] is int ? t['decimals'] as int : 0,
  );

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
    final deadline = _now().toUtc().add(quoteValidity);
    final request = <String, Object?>{
      'dry': false,
      'swapType': 'EXACT_INPUT',
      'originAsset': zecAssetId,
      'depositType': 'ORIGIN_CHAIN',
      'destinationAsset': asset.assetId,
      'amount': '$amountInZatoshi',
      'refundTo': refundTo,
      'refundType': 'ORIGIN_CHAIN',
      'recipient': recipient,
      'recipientType': 'DESTINATION_CHAIN',
      'deadline': deadline.toIso8601String(),
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
    return SwapQuote(
      depositAddress: _string(quote, 'depositAddress'),
      depositMemo: _optional(quote, 'depositMemo'),
      amountInZatoshi: amountInZatoshi,
      amountOut: _string(quote, 'amountOut'),
      asset: asset,
      // The provider's own deadline where it states one: honouring a longer
      // one of ours would quote a price it has stopped holding.
      deadline: _instant(quote['deadline']) ?? deadline,
      reference: _optional(body, 'correlationId'),
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
    return SwapStatus(
      state: _state(status),
      destinationTxHash:
          _optional(body, 'destinationTxHash') ??
          _optional(body, 'destinationChainTxHash'),
      detail: _optional(body, 'message'),
    );
  }

  /// The provider's own vocabulary, mapped onto §9.2's three answers.
  ///
  /// **An unrecognised status is [SwapState.processing], never
  /// [SwapState.delivered].** Reading an unknown word as success would tell a
  /// payer their debt is settled on the strength of a string nobody here has
  /// defined.
  static SwapState _state(String status) => switch (status) {
    'PENDING_DEPOSIT' || 'KNOWN_DEPOSIT_TX' => SwapState.awaitingDeposit,
    'SUCCESS' => SwapState.delivered,
    'FAILED' || 'REFUNDED' || 'EXPIRED' => SwapState.failed,
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

  static String? _optional(Map<String, dynamic> o, String key) {
    final v = o[key];
    return (v is String && v.isNotEmpty) ? v : null;
  }

  static DateTime? _instant(Object? raw) =>
      raw is String ? DateTime.tryParse(raw)?.toUtc() : null;
}
