/// A split being edited, and the §4 payload it becomes.
///
/// The protocol defines five ways to divide an expense and refuses a sixth.
/// A form cannot hold a §4 payload directly — half-typed input is not a split
/// — so this holds what a person has entered so far and answers two questions:
/// what would this become, and what is wrong with it now.
///
/// **The protocol decides what is wrong, not this file.** Every refusal here
/// comes from building the real payload and splitting a real expense with it,
/// so a form can never accept something the fold would set aside, and can
/// never invent a rule §4 does not have.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

/// The five, and only the five (§4).
enum SplitKind { equal, exact, percentage, shares, itemized }

extension SplitKindWire on SplitKind {
  /// The §4 discriminator.
  ///
  /// What a person is shown for each kind is the wallet's: a name written
  /// here would be English only, and every wallet that is not in English
  /// would carry a second one anyway.
  String get wireType => name;
}

/// One line of an itemized split (§4.5).
class DraftItem {
  DraftItem({this.description = '', this.minorUnits = 0, Set<String>? sharedBy})
    : sharedBy = sharedBy ?? <String>{};

  String description;
  int minorUnits;

  /// Who ate it. §4.5 refuses an item assigned to nobody.
  final Set<String> sharedBy;
}

/// What a person has entered, and what it would become.
class SplitDraft {
  SplitDraft({
    required this.kind,
    Set<String>? among,
    Map<String, int>? amounts,
    Map<String, int>? basisPoints,
    Map<String, int>? shareCounts,
    List<DraftItem>? items,
    this.extraMinorUnits = 0,
  }) : among = among ?? <String>{},
       amounts = amounts ?? <String, int>{},
       basisPoints = basisPoints ?? <String, int>{},
       shareCounts = shareCounts ?? <String, int>{},
       items = items ?? <DraftItem>[];

  SplitKind kind;

  /// Who shares it, for [SplitKind.equal].
  final Set<String> among;

  /// Minor units per participant, for [SplitKind.exact].
  final Map<String, int> amounts;

  /// Hundredths of a percent per participant, for [SplitKind.percentage].
  ///
  /// Basis points rather than percentages so a share is an integer at every
  /// step: 33.33% is 3333, and no double ever touches an amount.
  final Map<String, int> basisPoints;

  /// Weights per participant, for [SplitKind.shares].
  final Map<String, int> shareCounts;

  /// The lines, for [SplitKind.itemized].
  final List<DraftItem> items;

  /// Tax, tip or service, for [SplitKind.itemized]. Allocated across people
  /// in proportion to what they ate (§4.5).
  int extraMinorUnits;

  /// The §4 payload this would become.
  ///
  /// Built whatever state the form is in: it is the protocol's job to say a
  /// payload is wrong, and building only "valid" ones here would mean this
  /// file deciding what valid means.
  Map<String, dynamic> toSplit() => switch (kind) {
    SplitKind.equal => <String, dynamic>{
      'type': 'equal',
      'among': among.toList()..sort(),
    },
    SplitKind.exact => <String, dynamic>{
      'type': 'exact',
      'amounts': Map<String, dynamic>.from(amounts),
    },
    SplitKind.percentage => <String, dynamic>{
      'type': 'percentage',
      'basisPoints': Map<String, dynamic>.from(basisPoints),
    },
    SplitKind.shares => <String, dynamic>{
      'type': 'shares',
      'shareCounts': Map<String, dynamic>.from(shareCounts),
    },
    SplitKind.itemized => <String, dynamic>{
      'type': 'itemized',
      'extraMinorUnits': extraMinorUnits,
      'items': [
        for (final item in items)
          <String, dynamic>{
            'description': item.description,
            'minorUnits': item.minorUnits,
            'sharedBy': item.sharedBy.toList()..sort(),
          },
      ],
    },
  };

  /// What each participant would owe of [totalMinorUnits], or null when the
  /// protocol refuses this split.
  ///
  /// The allocation is the protocol's — largest remainder, exact integers —
  /// and is never reimplemented here. A form that did its own arithmetic
  /// would show a person a figure the bill then disagreed with.
  Map<String, int>? allocation(int totalMinorUnits) {
    try {
      return splitz.splitExpense(totalMinorUnits, toSplit());
    } on splitz.SplitError {
      return null;
    }
  }

  /// Why the protocol will not accept this yet, as its §12 code, or null when
  /// it will.
  ///
  /// The code, not a sentence. §1 says the code is what a wallet turns into a
  /// sentence for its user, and one written here would be English only.
  String? refusalCode(int totalMinorUnits) {
    try {
      splitz.splitExpense(totalMinorUnits, toSplit());
      return null;
    } on splitz.SplitError catch (e) {
      return e.code;
    }
  }

  /// Everyone this draft names, whichever kind it is.
  Set<String> get participants => switch (kind) {
    SplitKind.equal => among,
    SplitKind.exact => amounts.keys.toSet(),
    SplitKind.percentage => basisPoints.keys.toSet(),
    SplitKind.shares => shareCounts.keys.toSet(),
    SplitKind.itemized => {for (final item in items) ...item.sharedBy},
  };

  /// Puts [id] in or out of the split, whichever kind it is.
  ///
  /// Removing somebody drops the figure they carried rather than leaving it
  /// behind: a stale amount for a person no longer in the split is exactly
  /// what `exact_total_mismatch` is.
  void toggle(String id) {
    switch (kind) {
      case SplitKind.equal:
        if (!among.remove(id)) among.add(id);
      case SplitKind.exact:
        if (amounts.remove(id) == null) amounts[id] = 0;
      case SplitKind.percentage:
        if (basisPoints.remove(id) == null) basisPoints[id] = 0;
      case SplitKind.shares:
        if (shareCounts.remove(id) == null) shareCounts[id] = 1;
      case SplitKind.itemized:
        for (final item in items) {
          if (!item.sharedBy.remove(id)) item.sharedBy.add(id);
        }
    }
  }
}
