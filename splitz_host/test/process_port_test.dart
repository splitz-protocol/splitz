/// The helper every server-backed test starts its server through.
library;

import 'dart:io';

import 'package:test/test.dart';

import 'support/process_port.dart';

void main() {
  test(
    'a server that never reports a real port is refused and stopped',
    () async {
      // Prints the port it was asked for rather than the one it bound, then
      // stays up — the shape of a server that would otherwise outlive the run.
      final dir = await Directory.systemTemp.createTemp('port');
      addTearDown(() => dir.delete(recursive: true));
      final script = File('${dir.path}/zero.py')
        ..writeAsStringSync(
          'import sys, time\n'
          'print("listening on http://127.0.0.1:0", flush=True)\n'
          'time.sleep(600)\n',
        );
      final pids = <int>[];
      await expectLater(
        startOnFreePort(script.path, const []).then((up) {
          pids.add(up.process.pid);
          return up;
        }),
        throwsA(isA<StateError>()),
      );
      // Nothing named zero.py is left running.
      final ps = await Process.run('pgrep', ['-f', script.path]);
      expect((ps.stdout as String).trim(), isEmpty);
    },
  );
}
