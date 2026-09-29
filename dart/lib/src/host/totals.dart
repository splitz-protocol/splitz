/// Where this device stands with each person, summed over every bill it holds.
///
/// A bill settles on its own (§6), and one person may be on several. A wallet
/// listing bills can also say "you owe Ana 45.00 overall" — per person, per
/// currency, since amounts in two currencies are never added (§2.1).
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'bill_log.dart';

/// What this device and one other participant owe each other in one
/// currency, across every bill that names both.
class Standing {
  const Standing({
    required this.withId,
    required this.currency,
    required this.owedToMe,
    required this.owedByMe,
    required this.sentAwaiting,
    required this.receivedAwaiting,
    required this.billIds,
  });

  /// The other participant. One person is one id across bills only when the
  /// id derives from their key (§10.7); an unsigned participant is a
  /// different id on every bill.
  final String withId;
  final String currency;

  /// What their settlement plans ask them to pay this device, in minor units.
  final int owedToMe;

  /// What the plans ask this device to pay them.
  final int owedByMe;

  /// Payments this device recorded to them that they have not confirmed.
  /// Still in [owedByMe]: a record moves nothing until it is confirmed
  /// (§10.5), so this says what is already on its way.
  final int sentAwaiting;

  /// Payments they recorded to this device that it has not confirmed.
  final int receivedAwaiting;

  /// The bills this standing sums, in §2.3 order.
  final List<String> billIds;

  /// Positive when they owe this device on balance, negative when it owes
  /// them. Not a sum to settle by: each bill is settled on its own plan.
  int get net => splitz.checkedSubtract(owedToMe, owedByMe);
}

/// [standings], and the bills that could not be counted.
class Totals {
  const Totals({required this.standings, required this.uncounted});

  /// One per other participant and currency, in §2.3 order of id then
  /// currency. A pair with nothing owed and nothing awaiting either way is
  /// left out.
  final List<Standing> standings;

  /// Bills left out whole, by id, with the §12 code that kept each out: one
  /// that cannot be settled, or one whose amounts would carry a standing past
  /// what an amount can hold. Never partly counted.
  final Map<String, String> uncounted;
}

/// Sums what [me] owes and is owed across [bills] (§6, §10.5).
Totals totalsAcross(List<FoldedBill> bills, String me) {
  final sums = <String, List<int>>{};
  final billsOf = <String, Set<String>>{};
  final uncounted = <String, String>{};
  String key(String withId, String currency) => '$withId\u0000$currency';

  final ordered = [...bills]
    ..sort((a, b) => splitz.compareUtf8(a.bill.id, b.bill.id));
  for (final folded in ordered) {
    final bill = folded.bill;
    // Everything this bill adds, worked out before any of it is kept.
    final adds = <String, List<int>>{};
    void add(String withId, int slot, int amount) {
      final row =
          adds.putIfAbsent(key(withId, bill.currency), () => [0, 0, 0, 0]);
      row[slot] = splitz.checkedAdd(row[slot], amount);
    }

    try {
      for (final s in splitz.settleBill(bill).settlements) {
        if (s.to == me && s.from != me) add(s.from, 0, s.amount);
        if (s.from == me && s.to != me) add(s.to, 1, s.amount);
      }
      for (final p in bill.payments) {
        if (bill.confirmedPayments.contains(p.id)) continue;
        if (p.from == me && p.to != me) add(p.to, 2, p.amount);
        if (p.to == me && p.from != me) add(p.from, 3, p.amount);
      }
      final merged = <String, List<int>>{};
      for (final e in adds.entries) {
        final held = sums[e.key] ?? const [0, 0, 0, 0];
        merged[e.key] = [
          for (var i = 0; i < 4; i++) splitz.checkedAdd(held[i], e.value[i]),
        ];
      }
      for (final e in merged.entries) {
        sums[e.key] = e.value;
        billsOf.putIfAbsent(e.key, () => {}).add(bill.id);
      }
    } on splitz.SplitError catch (e) {
      uncounted[bill.id] = e.code;
    }
  }

  final standings = <Standing>[];
  final keys = sums.keys.toList()..sort(splitz.compareUtf8);
  for (final k in keys) {
    final row = sums[k]!;
    if (row.every((v) => v == 0)) continue;
    final cut = k.indexOf('\u0000');
    standings.add(Standing(
      withId: k.substring(0, cut),
      currency: k.substring(cut + 1),
      owedToMe: row[0],
      owedByMe: row[1],
      sentAwaiting: row[2],
      receivedAwaiting: row[3],
      billIds: splitz.sortedUtf8(billsOf[k]!),
    ));
  }
  return Totals(standings: standings, uncounted: uncounted);
}
