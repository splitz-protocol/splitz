/// Whether a refund accounts for a settlement's unexplained part (§6.3).
library;

import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

protocol.Expense _expense(
  String id,
  String paidBy,
  int amount,
  List<String> among,
) => protocol.Expense(
  id: id,
  description: id,
  paidBy: paidBy,
  amount: amount,
  currency: 'USD',
  at: '2026-10-28T19:30:00.000Z',
  split: {'type': 'equal', 'among': among},
);

FoldedBill _folded(
  List<protocol.Expense> expenses,
  Map<String, String> authors,
) => FoldedBill(
  bill: protocol.Bill(
    id: 'b',
    name: 'Trip',
    currency: 'USD',
    expenses: expenses,
  ),
  creatorId: 'ana',
  setAside: const [],
  withdrawn: const [],
  replacedAddresses: const [],
  identities: const protocol.Identities({}),
  expenseAuthors: authors,
);

void main() {
  test(
    'a refund that moves the whole unexplained part onto the payer names it',
    () {
      // Cara's refund of 10.00 is "paid" by me and shared by me and Ben: Ben's
      // -5.00 share moves onto me.
      final folded = _folded(
        [
          _expense('cara:r', 'me', -1000, ['me', 'ben']),
        ],
        {'cara:r': 'cara'},
      );
      final found = refundsBehind(
        const protocol.Settlement('me', 'ben', 500),
        folded,
      );
      expect(found?.refunded, 500);
      expect(found?.authors, ['cara']);
    },
  );

  test('an overpayment is not a refund', () {
    // No expense below zero: the unexplained part is a confirmed payment
    // above what was owed.
    final folded = _folded([
      _expense('me:d', 'me', 1000, ['me', 'ben']),
    ], {});
    expect(
      refundsBehind(const protocol.Settlement('me', 'ben', 500), folded),
      isNull,
    );
  });

  test(
    'a refund smaller than the unexplained part does not account for it',
    () {
      final folded = _folded(
        [
          _expense('cara:r', 'me', -200, ['me', 'ben']),
        ],
        {'cara:r': 'cara'},
      );
      expect(
        refundsBehind(const protocol.Settlement('me', 'ben', 500), folded),
        isNull,
      );
    },
  );

  test('a settlement its covers explain has nothing to account for', () {
    final folded = _folded(
      [
        _expense('cara:r', 'me', -1000, ['me', 'ben']),
      ],
      {'cara:r': 'cara'},
    );
    const covered = protocol.Settlement(
      'me',
      'ben',
      500,
      covers: [protocol.DirectDebt('me', 'ben', 500)],
    );
    expect(refundsBehind(covered, folded), isNull);
  });
}
