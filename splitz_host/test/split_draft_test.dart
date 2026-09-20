/// A split being edited, against the protocol that decides what it may be.
///
/// Every refusal asserted here is produced by building the real §4 payload and
/// splitting a real expense with it — so none of these codes can outlive the
/// rule that causes them, and none can be invented.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

void main() {
  group('each of the five reaches the protocol', () {
    test('equal', () {
      final draft = SplitDraft(kind: SplitKind.equal, among: {'ana', 'ben'});
      expect(draft.refusalCode(9000), isNull);
      expect(draft.allocation(9000), {'ana': 4500, 'ben': 4500});
    });

    test('equal, with a remainder the protocol places', () {
      // Largest remainder, exact integers: nobody loses a cent and the total
      // is preserved.
      final draft = SplitDraft(
        kind: SplitKind.equal,
        among: {'ana', 'ben', 'cai'},
      );
      final allocated = draft.allocation(1000)!;
      expect(allocated.values.reduce((a, b) => a + b), 1000);
      expect(allocated.values, containsAll([334, 333, 333]));
    });

    test('exact', () {
      final draft = SplitDraft(
        kind: SplitKind.exact,
        amounts: {'ana': 6000, 'ben': 3000},
      );
      expect(draft.refusalCode(9000), isNull);
      expect(draft.allocation(9000), {'ana': 6000, 'ben': 3000});
    });

    test('percentage, carried in basis points', () {
      // 33.33% is 3333. No double ever touches an amount.
      final draft = SplitDraft(
        kind: SplitKind.percentage,
        basisPoints: {'ana': 3333, 'ben': 6667},
      );
      expect(draft.refusalCode(9000), isNull);
      expect(draft.allocation(9000)!.values.reduce((a, b) => a + b), 9000);
    });

    test('shares', () {
      final draft = SplitDraft(
        kind: SplitKind.shares,
        shareCounts: {'ana': 2, 'ben': 1},
      );
      expect(draft.refusalCode(9000), isNull);
      expect(draft.allocation(9000), {'ana': 6000, 'ben': 3000});
    });

    test('itemized, with the extra spread over what people ate', () {
      final draft = SplitDraft(
        kind: SplitKind.itemized,
        items: [
          DraftItem(description: 'tacos', minorUnits: 6000, sharedBy: {'ana'}),
          DraftItem(
            description: 'beer',
            minorUnits: 2000,
            sharedBy: {'ana', 'ben'},
          ),
        ],
        extraMinorUnits: 1000,
      );
      expect(draft.refusalCode(9000), isNull);
      final allocated = draft.allocation(9000)!;
      expect(allocated.values.reduce((a, b) => a + b), 9000);
      // Ana ate more, so she carries more of the tip.
      expect(allocated['ana']!, greaterThan(allocated['ben']!));
    });
  });

  group('what the protocol refuses, by its code', () {
    test('nobody sharing it', () {
      final draft = SplitDraft(kind: SplitKind.equal);
      expect(draft.refusalCode(9000), splitz.SplitCode.emptySplit);
    });

    test('exact amounts that do not come to the total', () {
      final draft = SplitDraft(
        kind: SplitKind.exact,
        amounts: {'ana': 6000, 'ben': 2000},
      );
      expect(draft.refusalCode(9000), splitz.SplitCode.exactTotalMismatch);
      expect(draft.allocation(9000), isNull);
    });

    test('percentages that do not come to 100', () {
      final draft = SplitDraft(
        kind: SplitKind.percentage,
        basisPoints: {'ana': 3000, 'ben': 6000},
      );
      expect(draft.refusalCode(9000), splitz.SplitCode.percentageNotFullScale);
    });

    test('everybody on zero shares', () {
      final draft = SplitDraft(
        kind: SplitKind.shares,
        shareCounts: {'ana': 0, 'ben': 0},
      );
      expect(draft.refusalCode(9000), splitz.SplitCode.zeroWeightSum);
    });

    test('an itemized split with no items', () {
      final draft = SplitDraft(kind: SplitKind.itemized);
      expect(draft.refusalCode(9000), splitz.SplitCode.itemizedNoItems);
    });

    test('an item nobody shared', () {
      final draft = SplitDraft(
        kind: SplitKind.itemized,
        items: [DraftItem(description: 'tacos', minorUnits: 9000)],
      );
      expect(draft.refusalCode(9000), splitz.SplitCode.itemizedUnassignedItem);
    });

    test('items that do not come to the total', () {
      final draft = SplitDraft(
        kind: SplitKind.itemized,
        items: [
          DraftItem(description: 'tacos', minorUnits: 5000, sharedBy: {'ana'}),
        ],
        extraMinorUnits: 1000,
      );
      expect(draft.refusalCode(9000), splitz.SplitCode.itemizedTotalMismatch);
    });

    test('a share running the opposite way to the expense', () {
      final draft = SplitDraft(
        kind: SplitKind.exact,
        amounts: {'ana': 10000, 'ben': -1000},
      );
      expect(draft.refusalCode(9000), splitz.SplitCode.negativeShare);
    });
  });

  group('editing', () {
    test('toggling puts somebody in, and takes their figure back out', () {
      // A stale amount for a person no longer in the split is exactly what
      // `exact_total_mismatch` is.
      final draft = SplitDraft(kind: SplitKind.exact);
      draft.toggle('ana');
      expect(draft.amounts.containsKey('ana'), isTrue);
      draft.amounts['ana'] = 9000;
      draft.toggle('ana');
      expect(draft.amounts.containsKey('ana'), isFalse);
      expect(draft.participants, isEmpty);
    });

    test('somebody added to shares starts on one, not zero', () {
      // Zero shares owes nothing, and every count zero is refused outright.
      final draft = SplitDraft(kind: SplitKind.shares);
      draft.toggle('ana');
      expect(draft.shareCounts['ana'], 1);
      expect(draft.refusalCode(9000), isNull);
    });

    test('the payload is the protocol shape, whatever the form holds', () {
      final draft = SplitDraft(kind: SplitKind.equal, among: {'ben', 'ana'});
      final split = draft.toSplit();
      expect(split['type'], 'equal');
      // §4.1 sorts the surviving set; two devices building one split from one
      // form must produce one payload.
      expect(split['among'], ['ana', 'ben']);
    });

    test('every kind names the wire type the protocol reads', () {
      for (final kind in SplitKind.values) {
        final draft = SplitDraft(kind: kind);
        expect(draft.toSplit()['type'], kind.wireType);
      }
      // And the protocol knows all five: a sixth would be refused with
      // `bill_unknown_split_type`, so reaching any other refusal proves the
      // type itself was accepted.
      expect(SplitKind.values.length, 5);
      for (final kind in SplitKind.values) {
        try {
          splitz.splitExpense(9000, SplitDraft(kind: kind).toSplit());
        } on splitz.SplitError catch (e) {
          expect(
            e.code,
            isNot(splitz.SplitCode.billUnknownSplitType),
            reason: '${kind.wireType} is not a split this protocol defines',
          );
        }
      }
    });
  });
}
