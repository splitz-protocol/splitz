/// The five split methods (SPEC.md §4).
///
/// Every method produces shares summing exactly to the expense total.
/// Participants are ordered by ascending id (§2.3) before allocation, so the
/// leftover units of §3 land on the same people everywhere.
library;

import 'allocation.dart';
import 'errors.dart';
import 'money.dart';
import 'ordering.dart';

/// Resolves [total] into owed minor units per participant.
Map<String, int> splitExpense(int total, Map<String, dynamic> spec) {
  checkIdLists(spec);
  final type = spec['type'];
  switch (type) {
    case 'equal':
      return _equal(total, spec);
    case 'exact':
      return _exact(total, spec);
    case 'percentage':
      return _percentage(total, spec);
    case 'shares':
      return _shares(total, spec);
    case 'itemized':
      return _itemized(total, spec);
    default:
      raise(SplitCode.billUnknownSplitType, 'No such split method: $type');
  }
}

Map<String, int> _equal(int total, Map<String, dynamic> spec) {
  final among = uniqueSortedUtf8(_ids(spec['among']));
  if (among.isEmpty) {
    raise(SplitCode.emptySplit, 'An equal split names nobody');
  }
  return _zip(among, allocateEvenly(total, among.length));
}

/// Every participant id a split names, whatever method it uses.
///
/// Kept beside the split methods so a new method cannot add a place an id
/// hides. A caller checks these against the bill's participants; the ids are
/// returned rather than checked here because this file knows nothing about
/// which bill a split belongs to.
Set<String> splitParticipants(Map<String, dynamic> spec) {
  final out = <String>{};
  void addAll(Object? v) {
    if (v is List) {
      for (final x in v) {
        if (x is String) out.add(x);
      }
    }
  }

  addAll(spec['among']);
  for (final key in const ['amounts', 'basisPoints', 'shareCounts']) {
    final m = spec[key];
    if (m is Map) {
      for (final k in m.keys) {
        if (k is String) out.add(k);
      }
    }
  }
  final items = spec['items'];
  if (items is List) {
    for (final item in items) {
      if (item is Map) addAll(item['sharedBy']);
    }
  }
  return out;
}

/// Refuses an id list holding anything but strings (§10.1, §4).
///
/// Dropping a member a reader cannot read reassigns that participant's share
/// to the others: three people splitting 9000 become two paying 4500 each.
void checkIdLists(Map<String, dynamic> spec) {
  for (final key in const ['among', 'sharedBy']) {
    final list = spec[key];
    if (list is List) {
      for (final v in list) {
        if (v is! String) {
          raise(
              SplitCode.billTypeError, 'A $key names participants as strings');
        }
      }
    }
  }
  for (final key in const ['amounts', 'basisPoints', 'shareCounts']) {
    final map = spec[key];
    if (map is Map) {
      for (final k in map.keys) {
        if (k is! String) {
          raise(SplitCode.billTypeError, 'A $key is keyed by participant id');
        }
      }
    }
  }
  final items = spec['items'];
  if (items is List) {
    for (final item in items) {
      if (item is Map) checkIdLists(item.cast<String, dynamic>());
    }
  }
}

/// True when `value` carries the sign opposite to `total` (§4).
///
/// The rule is sign agreement rather than non-negativity because §10.4 makes a
/// refund an expense with a negative total, and every method must divide one.
bool _against(int value, int total) => total < 0 ? value > 0 : value < 0;

Map<String, int> _exact(int total, Map<String, dynamic> spec) {
  final amounts = _intMap(spec['amounts']);
  final ids = sortedUtf8(amounts.keys);
  for (final id in ids) {
    if (_against(amounts[id]!, total)) {
      raise(SplitCode.negativeShare,
          '$id is given a share pulling against a total of $total');
    }
  }
  final sum = checkedSum(ids.map((i) => amounts[i]!));
  if (sum != total) {
    raise(SplitCode.exactTotalMismatch,
        'The stated shares sum to $sum, not $total');
  }
  return {for (final id in ids) id: amounts[id]!};
}

Map<String, int> _percentage(int total, Map<String, dynamic> spec) {
  final points = _intMap(spec['basisPoints']);
  final ids = sortedUtf8(points.keys);
  for (final id in ids) {
    if (points[id]! < 0) {
      raise(SplitCode.negativeWeight, '$id is given negative basis points');
    }
  }
  // The overflow check precedes the full-scale check: four values of 2^62 sum
  // to zero in a 64-bit integer, and a fifth of 10000 then satisfies it.
  final sum = checkedSum(ids.map((i) => points[i]!));
  if (sum != 10000) {
    raise(SplitCode.percentageNotFullScale,
        'Basis points sum to $sum, not 10000');
  }
  return _zip(ids, allocate(total, [for (final id in ids) points[id]!]));
}

Map<String, int> _shares(int total, Map<String, dynamic> spec) {
  final counts = _intMap(spec['shareCounts']);
  final ids = sortedUtf8(counts.keys);
  return _zip(ids, allocate(total, [for (final id in ids) counts[id]!]));
}

Map<String, int> _itemized(int total, Map<String, dynamic> spec) {
  final items = spec['items'];
  if (items is! List || items.isEmpty) {
    raise(SplitCode.itemizedNoItems, 'An itemised split lists no items');
  }
  // §4.5: an item is an object. Typed before it is indexed, because `cast`
  // is lazy and would fail at the first member read instead.
  for (final item in items) {
    if (item is! Map) {
      raise(SplitCode.billTypeError, 'An item is an object, got $item');
    }
  }
  final list = items.cast<Map<String, dynamic>>();
  for (final item in list) {
    final who = _ids(item['sharedBy']);
    if (who.isEmpty) {
      raise(SplitCode.itemizedUnassignedItem,
          'An item is assigned to nobody: ${item['description']}');
    }
  }

  final extra = _int(spec['extraMinorUnits'] ?? 0);
  if (_against(extra, total)) {
    raise(SplitCode.negativeShare,
        'An extra of $extra pulls against a total of $total');
  }
  for (final item in list) {
    if (_against(_int(item['minorUnits']), total)) {
      raise(SplitCode.negativeShare, 'An item pulls against a total of $total');
    }
  }
  // These checks run in the order §4.5 states, so an input failing two of them
  // is refused with the same code everywhere.
  final stated =
      checkedSum([for (final i in list) _int(i['minorUnits']), extra]);
  if (stated != total) {
    raise(SplitCode.itemizedTotalMismatch,
        'Items and extra sum to $stated, not $total');
  }

  final subtotal = <String, int>{};
  for (final item in list) {
    final who = uniqueSortedUtf8(_ids(item['sharedBy']));
    final parts = allocateEvenly(_int(item['minorUnits']), who.length);
    for (var i = 0; i < who.length; i++) {
      subtotal[who[i]] = (subtotal[who[i]] ?? 0) + parts[i];
    }
  }

  final ids = sortedUtf8(subtotal.keys);
  if (extra != 0) {
    // Magnitudes: §3.1 refuses a negative weight and a refund's subtotals are
    // all negative. The proportion is the same either way.
    final weights = [for (final id in ids) subtotal[id]!.abs()];
    // Weights that are all zero have no proportion to preserve.
    final shares = weights.any((w) => w != 0)
        ? allocate(extra, weights)
        : allocateEvenly(extra, ids.length);
    for (var i = 0; i < ids.length; i++) {
      subtotal[ids[i]] = subtotal[ids[i]]! + shares[i];
    }
  }
  return {for (final id in ids) id: subtotal[id]!};
}

List<String> _ids(Object? value) =>
    value is List ? value.cast<String>() : const <String>[];

Map<String, int> _intMap(Object? value) {
  if (value is! Map) return const {};
  return {for (final e in value.entries) e.key as String: _int(e.value)};
}

int _int(Object? value) => documentInteger(value);

Map<String, int> _zip(List<String> ids, List<int> parts) =>
    {for (var i = 0; i < ids.length; i++) ids[i]: parts[i]};
