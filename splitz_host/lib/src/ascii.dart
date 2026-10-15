/// Text compared the one way every implementation compares it.
library;

/// Whether [a] and [b] are equal once ASCII letters are folded to one case,
/// every other character compared as it is: the one fold §9.1 and §15.7
/// compare an asset or a chain with, so every implementation agrees on a name
/// outside ASCII.
bool equalsIgnoringAsciiCase(String a, String b) {
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    var x = a.codeUnitAt(i), y = b.codeUnitAt(i);
    if (x >= 0x61 && x <= 0x7a) x -= 0x20;
    if (y >= 0x61 && y <= 0x7a) y -= 0x20;
    if (x != y) return false;
  }
  return true;
}
