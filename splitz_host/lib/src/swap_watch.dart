/// Swaps this device sent and has not seen finish.
///
/// A payment record carries the swap's `reference` and nothing else about the
/// provider (§9.2) — and that is deliberate: a deposit address is one
/// provider's routing detail for one swap, not something every participant
/// should carry on the bill forever.
///
/// Following one up still needs it, so it is kept here instead: **local to
/// the device that sent the swap**, never sealed, never synced, and deleted
/// once the swap is done. Nobody else needs it and nobody else is told it.
library;

import 'dart:convert';

import 'store.dart';
import 'swaps.dart';

/// What this device needs to ask a provider how a swap went.
class SwapWatch {
  const SwapWatch({
    required this.billId,
    required this.reference,
    required this.to,
    required this.depositAddress,
    required this.assetSymbol,
    required this.assetChain,
    this.depositMemo,
  });

  /// The bill the swap settles a debt on.
  final String billId;

  /// The payment record's `reference`, which is how the bill names it.
  final String reference;

  /// Who was owed.
  final String to;

  /// Where the ZEC was sent. The provider keys a status by this.
  final String depositAddress;

  /// Some chains lose a deposit sent without its memo, and a status query
  /// needs it too.
  final String? depositMemo;

  final String assetSymbol;
  final String assetChain;

  Map<String, dynamic> toJson() => <String, dynamic>{
    'billId': billId,
    'reference': reference,
    'to': to,
    'depositAddress': depositAddress,
    if (depositMemo != null) 'depositMemo': depositMemo,
    'assetSymbol': assetSymbol,
    'assetChain': assetChain,
  };

  static SwapWatch? fromJson(Object? raw) {
    if (raw is! Map<String, dynamic>) return null;
    String? str(String key) => raw[key] is String ? raw[key] as String : null;
    final billId = str('billId');
    final reference = str('reference');
    final to = str('to');
    final depositAddress = str('depositAddress');
    if (billId == null ||
        reference == null ||
        to == null ||
        depositAddress == null) {
      return null;
    }
    return SwapWatch(
      billId: billId,
      reference: reference,
      to: to,
      depositAddress: depositAddress,
      depositMemo: str('depositMemo'),
      assetSymbol: str('assetSymbol') ?? '',
      assetChain: str('assetChain') ?? '',
    );
  }

  /// The quote shape [SwapProvider.statusOf] asks for.
  ///
  /// Only the fields a status query reads are real; the amounts are not kept,
  /// because the bill already holds what was owed and what was sent and a
  /// second copy here would be a second thing to keep right.
  SwapQuote get asQuote => SwapQuote(
    depositAddress: depositAddress,
    depositMemo: depositMemo,
    amountInZatoshi: 0,
    amountOut: '',
    asset: TradableAsset(
      assetId: '',
      symbol: assetSymbol,
      chain: assetChain,
      decimals: 0,
    ),
    deadline: DateTime.utc(0),
    reference: reference,
  );
}

/// Keeps the swaps a device is still waiting on.
class SwapWatchList {
  const SwapWatchList(this._storage);

  final BillStorage _storage;

  /// Namespaced away from bills so a sweep of one never reaches the other.
  static const String _prefix = 'swapwatch/';

  String _key(String reference) => '$_prefix${Uri.encodeComponent(reference)}';

  Future<void> add(SwapWatch watch) =>
      _storage.write(_key(watch.reference), jsonEncode(watch.toJson()));

  /// Stops following [reference]. Called when a swap is confirmed or given
  /// up on: a list that only grows is one nobody reads.
  Future<void> forget(String reference) => _storage.delete(_key(reference));

  /// Everything still being followed, for [billId] when given.
  Future<List<SwapWatch>> held({String? billId}) async {
    final watches = <SwapWatch>[];
    for (final key in await _storage.keys(_prefix)) {
      final raw = await _storage.read(key);
      if (raw == null) continue;
      final Object? decoded;
      try {
        decoded = jsonDecode(raw);
      } on FormatException {
        // A damaged entry is skipped rather than thrown on: it costs a
        // follow-up, not a bill.
        continue;
      }
      final watch = SwapWatch.fromJson(decoded);
      if (watch == null) continue;
      if (billId != null && watch.billId != billId) continue;
      watches.add(watch);
    }
    return watches;
  }
}
