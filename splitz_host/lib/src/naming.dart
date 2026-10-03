/// What a participant is called on screen, and which names a reader cannot
/// tell apart (§9.1).
///
/// A display name is chosen by whoever joins and can be chosen twice, or
/// chosen to pass as somebody already on the bill. A collision cannot move
/// money; it can make the record of who did what unreadable. Every wallet
/// qualifies the same names the same way, so one bill reads alike on each.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

extension BillNaming on splitz.Bill {
  /// A participant's name, made unambiguous when more than one answers to it.
  ///
  /// A display name is chosen by whoever joins and can be chosen twice — by two
  /// friends both called Ana, or by somebody who read the invite and picked a
  /// name already on the bill. A collision cannot move money. What it can do is
  /// make the record of who did what unreadable, which is where the record's
  /// whole value is.
  ///
  /// Only a colliding name is qualified, so the ordinary case stays plain.
  String displayNameOf(String id, {String? creatorId}) {
    final person = participant(id);
    if (person == null) return shortId(id);
    final name = person.name.isEmpty ? shortId(id) : person.name;
    final alike = participants
        .where((p) => nameSkeleton(p.name) == nameSkeleton(person.name))
        .toList();
    if (alike.length < 2) return name;
    // The one who opened the bill is named as the organiser rather than by a
    // fragment of an id, because that is the half a reader can act on. Eight
    // characters otherwise: a key's id is a digest (§10.7), and matching
    // eight of its characters takes 2^48 tries where four take 2^24. An id
    // nobody's key derives is chosen freely, so one that copies another's
    // eight is shown whole: ids are unique on a bill.
    if (id == creatorId) return '$name (organiser)';
    final tail = shortId(id, length: 8);
    final copied = alike.any(
      (p) => p.id != id && shortId(p.id, length: 8) == tail,
    );
    return '$name (${copied ? id : tail})';
  }

  /// Every display name more than one participant answers to, in order.
  ///
  /// Two names count as one when a reader cannot tell them apart: see
  /// [nameSkeleton].
  List<String> get sharedNames {
    final first = <String, String>{};
    final shared = <String>{};
    for (final p in participants) {
      if (p.name.isEmpty) continue;
      final skeleton = nameSkeleton(p.name);
      final held = first[skeleton];
      if (held == null) {
        first[skeleton] = p.name;
      } else {
        shared
          ..add(held)
          ..add(p.name);
      }
    }
    // §2.3's order, so every implementation lists them alike.
    return splitz.sortedUtf8(shared);
  }

  /// The tail of an id, for a reader who needs to tell two apart: its last
  /// [length] characters, counted as Unicode scalar values (§2.3), or the
  /// whole id when that is hardly longer.
  static String shortId(String id, {int length = 4}) {
    final runes = id.runes.toList();
    if (runes.length <= length + 2) return id;
    return '…${String.fromCharCodes(runes.sublist(runes.length - length))}';
  }
}

/// [name] as a reader sees it, for telling whether two names can be told apart.
///
/// Case folded within [_caseFolded] — Latin, Greek, Cyrillic, Armenian and Latin
/// Extended Additional, less the few pairs encoded after the rest — and
/// nowhere else, so every implementation folds a name alike whatever Unicode
/// version its library carries; invisible format characters and combining
/// marks removed; runs
/// of space collapsed; and the Cyrillic and Greek letters that render as Latin
/// ones mapped to them. A look-alike chosen to pass as somebody already on the
/// bill then collides with them, and both are qualified. Not every confusable
/// Unicode knows: the common ones a name would be forged with.
String nameSkeleton(String name) {
  final out = StringBuffer();
  var space = false;
  final folded = [
    for (final r in name.runes)
      if (_caseFolded.any((range) => r >= range.$1 && r <= range.$2))
        ...String.fromCharCode(r).toLowerCase().runes
      else
        r,
  ];
  for (final rune in folded) {
    if (_invisible(rune) || _combining(rune)) continue;
    if (rune == 0x20 || rune == 0x09 || rune == 0xA0 || rune == 0x3000) {
      space = out.isNotEmpty;
      continue;
    }
    if (space) {
      out.write(' ');
      space = false;
    }
    out.writeCharCode(_lookAlike(rune));
  }
  return out.toString();
}

/// The code points [nameSkeleton] lower-cases, as inclusive ranges.
const List<(int, int)> _caseFolded = [
  (0x0000, 0x024F),
  (0x0370, 0x037E),
  (0x0380, 0x0523),
  (0x0531, 0x0556),
  (0x1E00, 0x1FFF),
];

bool _invisible(int r) =>
    r == 0xAD ||
    r == 0x34F ||
    r == 0x61C ||
    (r >= 0x115F && r <= 0x1160) ||
    (r >= 0x17B4 && r <= 0x17B5) ||
    (r >= 0x180B && r <= 0x180F) ||
    (r >= 0x200B && r <= 0x200F) ||
    (r >= 0x202A && r <= 0x202E) ||
    (r >= 0x2060 && r <= 0x206F) ||
    r == 0x3164 ||
    (r >= 0xFE00 && r <= 0xFE0F) ||
    r == 0xFEFF ||
    r == 0xFFA0 ||
    (r >= 0xE0000 && r <= 0xE0FFF);

bool _combining(int r) =>
    (r >= 0x300 && r <= 0x36F) ||
    (r >= 0x1AB0 && r <= 0x1AFF) ||
    (r >= 0x1DC0 && r <= 0x1DFF) ||
    (r >= 0x20D0 && r <= 0x20FF) ||
    (r >= 0xFE20 && r <= 0xFE2F);

/// Lower-case Cyrillic and Greek letters that render as a Latin one.
const Map<int, int> _latinLookAlike = {
  0x430: 0x61, // а
  0x432: 0x62, // в, read as a small-caps b
  0x435: 0x65, // е
  0x456: 0x69, // і
  0x458: 0x6A, // ј
  0x43A: 0x6B, // к
  0x43C: 0x6D, // м
  0x43D: 0x68, // н
  0x43E: 0x6F, // о
  0x440: 0x70, // р
  0x441: 0x63, // с
  0x442: 0x74, // т
  0x443: 0x79, // у
  0x445: 0x78, // х
  0x455: 0x73, // ѕ
  0x4CF: 0x6C, // ӏ
  0x3B1: 0x61, // α
  0x3B5: 0x65, // ε
  0x3B9: 0x69, // ι
  0x3BA: 0x6B, // κ
  0x3BD: 0x76, // ν
  0x3BF: 0x6F, // ο
  0x3C1: 0x70, // ρ
  0x3C4: 0x74, // τ
  0x3C5: 0x75, // υ
  0x3C7: 0x78, // χ
  0x131: 0x69, // ı
  0x1C0: 0x6C, // ǀ
  0x251: 0x61, // ɑ
  0x261: 0x67, // ɡ
  0x269: 0x69, // ɩ
};

/// The Latin letters U+00C0..=U+0233 that decompose to a plain letter and
/// combining marks, folded to that letter: index `r - 0xC0`, `.` where none.
/// The decomposed spelling of the same letter loses its marks in
/// [_combining], so both spellings of one name meet.
const String _precomposedBase =
    'aaaaaa.ceeeeiiii.nooooo..uuuuy..aaaaaa.ceeeeiiii.nooooo..uuuuy'
    '.yaaaaaaccccccccdd..eeeeeeeeeegggggggghh..iiiiiiiii...jjkk.lll'
    'lll....nnnnnn...oooooo..rrrrrrsssssssstttt..uuuuuuuuuuuuwwyyyz'
    'zzzzz.................................oo.............uu.......'
    '.....................aaiioouuuuuuuuuu.aaaa....ggkkoooo..j...gg'
    '..nnaa....aaaaeeeeiiiioooorrrruuuusstt..hh......aaeeooooooooyy';

/// [rune] as the plain letter or digit it renders as, when it is one.
///
/// Fullwidth forms are ASCII at another width; the mathematical alphanumerics
/// (bold, italic, script, fraktur, double-struck, sans-serif, monospace) are
/// letters in runs of 52, capitals then small, and digits in runs of 10.
int _lookAlike(int rune) {
  var r = rune;
  if (r >= 0xFF01 && r <= 0xFF5E) {
    r -= 0xFEE0;
  } else if (r >= 0x1D400 && r <= 0x1D6A3) {
    final i = (r - 0x1D400) % 52;
    r = 0x61 + (i < 26 ? i : i - 26);
  } else if (r >= 0x1D7CE && r <= 0x1D7FF) {
    r = 0x30 + (r - 0x1D7CE) % 10;
  }
  if (r >= 0x41 && r <= 0x5A) r += 0x20;
  if (r >= 0xC0 && r < 0xC0 + _precomposedBase.length) {
    final base = _precomposedBase.codeUnitAt(r - 0xC0);
    if (base != 0x2E) r = base;
  }
  return _latinLookAlike[r] ?? r;
}
