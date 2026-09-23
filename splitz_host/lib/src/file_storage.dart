/// Storage that survives the app being closed.
library;

import 'dart:convert';
import 'dart:io';

import 'store.dart';
import 'wallet.dart';

/// Keeps each bill in its own file under a directory the app chooses.
///
/// The directory is passed in rather than discovered, so this package takes no
/// dependency on the plugin that knows where an app may write. A wallet already
/// has one.
///
/// One file per bill, not one file holding all of them. A single file is
/// rewritten whole on every change, so a process that dies mid-write loses
/// every bill instead of one — and the bill being written is the one least
/// likely to be recoverable from a peer.
class FileBillStorage implements ScopedBillStorage {
  FileBillStorage(this.directory);

  final Directory directory;

  /// The directory's absolute path: two storages over one directory share
  /// one queue of writes per bill.
  @override
  Object get scope => directory.absolute.path;

  /// A stored name, as a file name that maps back to it.
  ///
  /// Every character outside `A–Z a–z 0–9 _ . -` is written as `%` and two
  /// upper-case hex digits per UTF-8 byte, `%` included, so the mapping is
  /// reversible: [keys] lists what was stored, not a lossy copy of it. A bill
  /// key is base64url and passes through unchanged; a `/`, which would write
  /// outside this directory, does not.
  static String _encode(String key) {
    final out = StringBuffer();
    for (final byte in utf8.encode(key)) {
      final c = String.fromCharCode(byte);
      if (RegExp(r'^[A-Za-z0-9_.\-]$').hasMatch(c)) {
        out.write(c);
      } else {
        out.write('%${byte.toRadixString(16).toUpperCase().padLeft(2, '0')}');
      }
    }
    return out.toString();
  }

  /// The stored name a file name encodes, or null for one [_encode] never
  /// writes.
  static String? _decode(String name) {
    final bytes = <int>[];
    for (var i = 0; i < name.length; i++) {
      final c = name[i];
      if (c == '%') {
        if (i + 2 >= name.length) return null;
        final byte = int.tryParse(name.substring(i + 1, i + 3), radix: 16);
        if (byte == null) return null;
        bytes.add(byte);
        i += 2;
      } else if (RegExp(r'^[A-Za-z0-9_.\-]$').hasMatch(c)) {
        bytes.add(c.codeUnitAt(0));
      } else {
        return null;
      }
    }
    try {
      return utf8.decode(bytes);
    } on FormatException {
      return null;
    }
  }

  File _fileFor(String key) => File('${directory.path}/${_encode(key)}');

  @override
  Future<String?> read(String key) async {
    final file = _fileFor(key);
    if (!await file.exists()) return null;
    try {
      return await file.readAsString();
    } on FileSystemException {
      // Unreadable is the same as absent to a caller: the bill is not here.
      // Raising would take down whatever listed the bills.
      return null;
    }
  }

  @override
  Future<void> write(String key, String value) async {
    await directory.create(recursive: true);
    // Written beside the target and renamed over it. A rename within one
    // filesystem is atomic, so a process that dies mid-write leaves either the
    // old contents or the new, never half of either — and half a log is a bill
    // that no longer folds. The temporary name is unique to this write: two
    // writers sharing one would truncate each other's file and one rename
    // would fail.
    final target = _fileFor(key);
    final temporary = File(_unfinished(target.path));
    await temporary.writeAsString(value, flush: true);
    await temporary.rename(target.path);
  }

  @override
  Future<void> delete(String key) async {
    final file = _fileFor(key);
    if (await file.exists()) await file.delete();
  }

  @override
  Future<List<String>> keys(String prefix) async {
    if (!await directory.exists()) return const [];
    final names = <String>[];
    await for (final entity in directory.list(followLinks: false)) {
      if (entity is! File) continue;
      // A half-written file is not a bill, so a crash during a write cannot
      // make a torn log look like a stored one.
      final name = entity.uri.pathSegments.last;
      if (_isUnfinished(name)) continue;
      final key = _decode(name);
      if (key != null && key.startsWith(prefix)) names.add(key);
    }
    names.sort();
    return names;
  }

  /// Removes any file left behind by a write that did not finish.
  ///
  /// Safe to call at startup and harmless when there is nothing to remove.
  /// A leftover is already invisible to [keys]; this stops them accumulating.
  @override
  Future<int> sweepUnfinishedWrites() async {
    if (!await directory.exists()) return 0;
    var removed = 0;
    await for (final entity in directory.list(followLinks: false)) {
      if (entity is File && _isUnfinished(entity.uri.pathSegments.last)) {
        await entity.delete();
        removed++;
      }
    }
    return removed;
  }
}

/// A temporary name beside [path], unique to one write.
String _unfinished(String path) =>
    '$path~${DateTime.now().microsecondsSinceEpoch}-'
    '${_writes++}.writing';
int _writes = 0;

/// A name a write left behind: this version's, or `<name>.writing` from one
/// that used a single temporary name. No stored key ends so.
bool _isUnfinished(String name) => name.endsWith('.writing');

/// A secret store backed by a file, for a build with no keychain.
///
/// **Not for a shipped build.** A bill key in a file is a bill key anything
/// that can read the file can use, and on a phone that is every process that
/// gets at the app's sandbox after it is unlocked. A wallet has a keychain and
/// should hand one in; this exists so a development run on a desktop can hold
/// keys across a restart.
class FileSecretStore implements SecretStore {
  FileSecretStore(this.file);

  final File file;

  Future<Map<String, String>> _read() async {
    if (!await file.exists()) return {};
    try {
      final decoded = jsonDecode(await file.readAsString());
      if (decoded is! Map) return {};
      return {
        for (final entry in decoded.entries)
          if (entry.value is String) '${entry.key}': entry.value as String,
      };
    } on Object {
      return {};
    }
  }

  Future<void> _write(Map<String, String> values) async {
    await file.parent.create(recursive: true);
    final temporary = File(_unfinished(file.path));
    await temporary.writeAsString(jsonEncode(values), flush: true);
    await temporary.rename(file.path);
  }

  /// Changes to the file, one at a time. Each reads the whole map and writes
  /// it back, so two at once would each drop the other's key.
  Future<void> _serial(void Function(Map<String, String>) change) {
    final run = _tail.then((_) async {
      final values = await _read();
      change(values);
      await _write(values);
    });
    _tail = run.catchError((Object _) {});
    return run;
  }

  Future<void> _tail = Future<void>.value();

  @override
  Future<String?> read(String key) async {
    await _tail;
    return (await _read())[key];
  }

  @override
  Future<void> write(String key, String value) =>
      _serial((values) => values[key] = value);

  @override
  Future<void> delete(String key) => _serial((values) => values.remove(key));
}
