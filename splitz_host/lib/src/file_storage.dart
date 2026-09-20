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
class FileBillStorage implements BillStorage {
  FileBillStorage(this.directory);

  final Directory directory;

  /// A stored name, made safe to be a file name.
  ///
  /// Keys are this package's own — a prefix and a bill id, and a bill id is
  /// base64url — so the mapping is close to identity. It is applied anyway
  /// because the one character base64url uses that a file name should not is
  /// `/`, and a key carrying one would write outside this directory.
  File _fileFor(String key) {
    final safe = key.replaceAll(RegExp(r'[^A-Za-z0-9_.-]'), '_');
    return File('${directory.path}/$safe');
  }

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
    // that no longer folds.
    final target = _fileFor(key);
    final temporary = File('${target.path}.writing');
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
      final name = entity.uri.pathSegments.last;
      // A half-written file is not a bill. Skipped rather than listed, so a
      // crash during a write cannot make a torn log look like a stored one.
      if (name.endsWith('.writing')) continue;
      if (name.startsWith(prefix)) names.add(name);
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
      if (entity is File && entity.path.endsWith('.writing')) {
        await entity.delete();
        removed++;
      }
    }
    return removed;
  }
}

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
    final temporary = File('${file.path}.writing');
    await temporary.writeAsString(jsonEncode(values), flush: true);
    await temporary.rename(file.path);
  }

  @override
  Future<String?> read(String key) async => (await _read())[key];

  @override
  Future<void> write(String key, String value) async => _write(
    await _read()
      ..[key] = value,
  );

  @override
  Future<void> delete(String key) async => _write(
    await _read()
      ..remove(key),
  );
}
