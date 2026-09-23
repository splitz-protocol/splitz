import 'dart:async';
import 'dart:convert';
import 'dart:io';

/// Starts a local server on port 0 and returns it with the port it bound.
///
/// The OS picks a free port and the server prints the one it bound, so the
/// port is read from its own output rather than chosen here and probed: a
/// probe of a chosen port can reach another test's server and pass for this
/// one coming up.
///
/// Both output streams are drained to the end. A pipe nobody reads closes
/// under the server, and the request that wrote the next log line fails.
Future<({Process process, int port})> startOnFreePort(
  String script,
  List<String> args,
) async {
  final process = await Process.start('python3', [
    script,
    ...args,
    '--port',
    '0',
  ]);
  final bound = Completer<int>();
  final seen = StringBuffer();
  void read(String line) {
    seen.writeln(line);
    final match = RegExp(r'http://127\.0\.0\.1:(\d+)').firstMatch(line);
    if (match != null && !bound.isCompleted) {
      bound.complete(int.parse(match.group(1)!));
    }
  }

  var open = 2;
  void done() {
    if (--open == 0 && !bound.isCompleted) {
      bound.completeError(
        StateError('$script exited before listening:\n$seen'),
      );
    }
  }

  for (final stream in [process.stdout, process.stderr]) {
    stream
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(read, onDone: done);
  }
  return (process: process, port: await bound.future);
}
