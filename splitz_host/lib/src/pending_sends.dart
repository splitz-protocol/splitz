/// Sends this device started and has not seen resolved (SPEC.md §14.3).
///
/// A send can reach the network, be refused before anything is built, or be
/// built and signed and not handed to the network — and the third may still
/// land. So may one the app died during. In either case the bill holds no
/// record of it, and without a note kept outside the bill the same debt is
/// offered again and the same money goes out twice.
///
/// The note is written **before** the wallet is called, kept in this device's
/// [BillStorage] under its own prefix, and blocks every further send from
/// that bill until somebody says which way it went.
library;

import 'dart:convert';

import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;

import 'store.dart';
import 'swap_watch.dart';

/// Which of §14.3's three outcomes a send ended in.
enum SendEnded {
  /// The transaction reached the network.
  reachedNetwork,

  /// Nothing was built, or nothing was spent.
  refused,

  /// Built and signed and not known to have reached the network — or the
  /// send raised, so which way it went is unknown. It may still land.
  unresolved,
}

/// One send from one bill, written down before the wallet was called.
class PendingSend {
  const PendingSend({
    required this.billId,
    required this.uri,
    required this.carried,
    required this.at,
    this.sent = const {},
    this.rate,
    this.swap,
    this.zatoshi,
    this.txid,
  });

  /// What [PendingSends.of] answers for a note that is there and will not
  /// read. It blocks exactly as a readable one does, and carries nothing to
  /// record from.
  const PendingSend.damaged(this.billId)
    : uri = '',
      carried = const {},
      at = '',
      sent = const {},
      rate = null,
      swap = null,
      zatoshi = null,
      txid = null;

  final String billId;

  /// The ZIP 321 request handed to the wallet. Empty only when [damaged].
  final String uri;

  /// What the request pays each recipient, in the bill's minor units: what a
  /// record of this send carries once its transaction is known.
  final Map<String, int> carried;

  /// When the send was started, as §9.3 renders an instant.
  final String at;

  /// What the request sends each recipient, in zatoshi: the ZEC a record of
  /// this send states (§9.2).
  final Map<String, int> sent;

  /// The rate the request was priced at: what a record states as
  /// `paidAtRate`.
  final splitz.ExchangeRate? rate;

  /// The swap this send was the deposit for, or null for a payment request.
  /// A swap is recorded by the provider's reference, not a transaction id.
  final SwapWatch? swap;

  /// What a swap's deposit sent, in zatoshi.
  final int? zatoshi;

  /// The transaction, when the wallet named one: the one an unresolved send
  /// built, or one that reached the network while its records could not be
  /// written.
  final String? txid;

  /// True for a note that would not read. Nothing can be recorded from it;
  /// a person records what they paid by hand, then [PendingSends.resolve]s.
  bool get damaged => uri.isEmpty;

  /// Whether [other] is the note of this same send: one [PendingSends.begin]
  /// wrote once, however its transaction was filled in since. A damaged note
  /// is nobody's.
  bool isSameSend(PendingSend other) =>
      !damaged && !other.damaged && at == other.at && uri == other.uri;

  /// This send, with the transaction [id] it went out as.
  PendingSend sentAs(String? id) => PendingSend(
    billId: billId,
    uri: uri,
    carried: carried,
    at: at,
    sent: sent,
    rate: rate,
    swap: swap,
    zatoshi: zatoshi,
    txid: id ?? txid,
  );

  Map<String, dynamic> toJson() => {
    'billId': billId,
    'uri': uri,
    'carried': carried,
    'at': at,
    if (sent.isNotEmpty) 'sent': sent,
    if (rate != null) 'rate': splitz.rateToJson(rate!),
    if (swap != null) 'swap': swap!.toJson(),
    if (zatoshi != null) 'zatoshi': zatoshi,
    if (txid != null) 'txid': txid,
  };

  /// Null for anything [toJson] did not write.
  static PendingSend? fromJson(Object? json) {
    if (json is! Map<String, dynamic>) return null;
    final billId = json['billId'];
    final uri = json['uri'];
    final at = json['at'];
    if (billId is! String || uri is! String || uri.isEmpty || at is! String) {
      return null;
    }
    final carried = _ints(json['carried']);
    final sent = _ints(json['sent'] ?? const <String, dynamic>{});
    if (carried == null || sent == null) return null;
    splitz.ExchangeRate? rate;
    if (json['rate'] != null) {
      try {
        rate = splitz.decodeRate(json['rate']);
      } on splitz.SplitError {
        return null;
      }
    }
    final swapJson = json['swap'];
    final swap = swapJson == null ? null : SwapWatch.fromJson(swapJson);
    if (swapJson != null && swap == null) return null;
    final zatoshi = json['zatoshi'];
    final txid = json['txid'];
    if (zatoshi != null && zatoshi is! int) return null;
    if (txid != null && txid is! String) return null;
    return PendingSend(
      billId: billId,
      uri: uri,
      carried: carried,
      at: at,
      sent: sent,
      rate: rate,
      swap: swap,
      zatoshi: zatoshi as int?,
      txid: txid as String?,
    );
  }

  static Map<String, int>? _ints(Object? json) {
    if (json is! Map<String, dynamic>) return null;
    final out = <String, int>{};
    for (final e in json.entries) {
      final v = e.value;
      if (v is! int) return null;
      out[e.key] = v;
    }
    return out;
  }
}

/// A send was refused because an earlier one from the same bill is not
/// resolved, or is still under way in this process.
class SendInFlight implements Exception {
  const SendInFlight(this.pending);

  /// The earlier send, when one is written down; null when it is still under
  /// way in this process and has not been.
  final PendingSend? pending;

  @override
  String toString() => 'SendInFlight(${pending?.billId ?? 'under way'})';
}

/// Why [PendingSends.recordsFor] cannot record a send.
enum UnrecordableReason {
  /// The text is not 64 hexadecimal digits, which is how a Zcash transaction
  /// id is written.
  notATransactionId,

  /// The note would not read, or carries nothing to record.
  detailsLost,

  /// The send was a swap's deposit, which is recorded by the provider's
  /// reference and not by a transaction id.
  isASwap,
}

class Unrecordable implements Exception {
  const Unrecordable(this.reason);

  final UnrecordableReason reason;

  @override
  String toString() => 'Unrecordable(${reason.name})';
}

/// The unresolved send for each bill, at most one per bill.
///
/// **One instance per storage.** The check that a send is not already under
/// way in this process is held by the instance.
class PendingSends {
  PendingSends(this._storage);

  final BillStorage _storage;

  /// Bills with a send between [begin] and [end], per storage and for the
  /// whole process: two instances over one storage — a feature closed and
  /// opened again while the wallet is still proving — are one wallet
  /// sending, and each on its own would let the other clear the note and
  /// send the debt again. [FileBillStorage] is one object per directory, so
  /// one directory is one set.
  static final Expando<Set<String>> _underWayOf = Expando();

  Set<String> get _underWay => _underWayOf[_storage] ??= <String>{};

  /// Namespaced away from bills so a sweep of one never reaches the other.
  static const String _prefix = 'pendingsend/';

  String _key(String billId) => '$_prefix${Uri.encodeComponent(billId)}';

  /// The unresolved send for [billId], or null.
  ///
  /// **A note that will not read still blocks.** Treating it as absent would
  /// let the send it stands for go out a second time, so it is answered as
  /// [PendingSend.damaged].
  Future<PendingSend?> of(String billId) async {
    final String? raw;
    try {
      raw = await _storage.read(_key(billId));
    } on BillStorageUnreadable {
      return PendingSend.damaged(billId);
    }
    if (raw == null) return null;
    Object? decoded;
    try {
      decoded = jsonDecode(raw);
    } on FormatException {
      decoded = null;
    }
    final send = PendingSend.fromJson(decoded);
    return send != null && send.billId == billId
        ? send
        : PendingSend.damaged(billId);
  }

  /// Writes [send] down before the wallet is called.
  ///
  /// Throws [SendInFlight] when a send from the same bill is written down and
  /// not resolved, or is between [begin] and [end] in this process. Every
  /// call that returns MUST be followed by [end], whatever the wallet did —
  /// including when it raised.
  Future<void> begin(PendingSend send) async {
    if (send.damaged) {
      throw ArgumentError.value(send, 'send', 'carries no request');
    }
    if (!_underWay.add(send.billId)) throw const SendInFlight(null);
    try {
      final held = await of(send.billId);
      if (held != null) throw SendInFlight(held);
      await _storage.write(_key(send.billId), jsonEncode(send.toJson()));
    } catch (_) {
      _underWay.remove(send.billId);
      rethrow;
    }
  }

  /// Settles what the note says once the wallet has answered (§14.3).
  ///
  /// [wrote] is the note [begin] was given. When it is, only that note is
  /// changed: one a later send wrote is neither deleted nor rewritten, and
  /// when the note is gone an unresolved send with a [txid] writes it back,
  /// so a transaction the wallet built always has a note naming it.
  ///
  /// - [SendEnded.reachedNetwork] and [recorded]: the bill holds the records,
  ///   so the note goes.
  /// - [SendEnded.reachedNetwork] and not [recorded] — the bill was forgotten
  ///   while the money went out: the note stays, with [txid], so the records
  ///   can be written once the bill is back.
  /// - [SendEnded.refused]: nothing was spent, so the note goes and the debt
  ///   can be sent again.
  /// - [SendEnded.unresolved]: the note stays, with [txid] when the wallet
  ///   named the transaction it built. It is what a person looks up to learn
  ///   which way the send went.
  Future<void> end(
    String billId,
    SendEnded how, {
    String? txid,
    bool recorded = false,
    PendingSend? wrote,
  }) async {
    try {
      final held = await of(billId);
      final ours = wrote == null || held == null || held.isSameSend(wrote);
      if (!ours) return;
      final clear =
          how == SendEnded.refused ||
          (how == SendEnded.reachedNetwork && recorded);
      if (clear) {
        if (held != null) await _storage.delete(_key(billId));
        return;
      }
      if (txid == null) return;
      final kept = held ?? (how == SendEnded.unresolved ? wrote : null);
      if (kept != null && !kept.damaged) {
        await _storage.write(
          _key(billId),
          jsonEncode(kept.sentAs(txid).toJson()),
        );
      }
    } finally {
      _underWay.remove(billId);
    }
  }

  /// Whether a send from [billId] is between [begin] and [end] in this
  /// process.
  bool underWay(String billId) => _underWay.contains(billId);

  /// Removes the note for [billId]: its records are on the bill, or a person
  /// has said nothing left the wallet.
  ///
  /// Throws [SendInFlight] while a send from [billId] is under way: until the
  /// wallet answers, nobody knows that nothing left it. When [seen] is given,
  /// only that note is removed, never one a later send wrote.
  Future<void> resolve(String billId, {PendingSend? seen}) async {
    if (_underWay.contains(billId)) throw const SendInFlight(null);
    if (seen != null) {
      final held = await of(billId);
      // A note that will not read names no send, so it is nobody's to keep:
      // a person who records what they paid by hand clears it.
      if (held != null && !held.damaged && !held.isSameSend(seen)) {
        throw SendInFlight(held);
      }
    }
    await _storage.delete(_key(billId));
  }

  /// The payment records for [send] having gone out as the transaction
  /// [txid], signed and appended to [log] — to merge into the bill before
  /// [resolve]. Recipients the bill already holds a record for under this
  /// transaction are left out: a second record under one payment id is a
  /// duplicate the fold sets aside.
  ///
  /// [txid] is trimmed and lower-cased. Throws [Unrecordable] when it is not
  /// 64 hexadecimal digits, when [send] is [PendingSend.damaged] or carries
  /// nothing, and when it was a swap's deposit.
  Future<List<Map<String, dynamic>>> recordsFor(
    BillHost host,
    BillLog log,
    PendingSend send,
    String txid,
  ) async {
    final id = txid.trim().toLowerCase();
    if (!RegExp(r'^[0-9a-f]{64}$').hasMatch(id)) {
      throw const Unrecordable(UnrecordableReason.notATransactionId);
    }
    if (send.damaged || send.carried.isEmpty) {
      throw const Unrecordable(UnrecordableReason.detailsLost);
    }
    if (send.swap != null) {
      throw const Unrecordable(UnrecordableReason.isASwap);
    }
    final recorded = {for (final p in log.fold().bill.payments) p.id};
    final carried = {
      for (final e in send.carried.entries)
        if (!recorded.contains(paymentIdForSend(host.me, id, e.key)))
          e.key: e.value,
    };
    return recordSend(
      host,
      log,
      carried,
      id,
      zatoshi: send.sent,
      rate: send.rate,
    );
  }
}
