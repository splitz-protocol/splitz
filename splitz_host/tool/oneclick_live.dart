// The swap client against the live 1Click API.
//
//   dart run tool/oneclick_live.dart check
//   dart run tool/oneclick_live.dart capture ../tools/contracts/fixtures
//
// `check` sends what `OneClickSwaps` builds, with `dry` set to true and
// nothing else changed, and reads the answers through the client's own
// readers: the token list must carry ZEC on its own chain, the quote must be
// accepted, and a status for an address the provider never issued must be a
// 404. It moves nothing and creates no deposit address. Exit 0 or 1.
//
// `capture` writes the provider's raw answers as fixtures for the tests: the
// token list, a dry quote, a live quote (`dry: false`, which issues a deposit
// address and moves nothing until somebody deposits), the status of that
// address, the status of an unknown one, and the refusal of a quote missing a
// required field.
//
// Origin: https://1click.chaindefuser.com, the provider's public API. It
// answers these three calls without credentials.
import 'dart:convert';
import 'dart:io';

import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';

const origin = 'https://1click.chaindefuser.com';
const zecAssetId = 'nep141:zec.omft.near';
const usdcOnBase =
    'nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near';
// Syntactically valid addresses nobody here controls; a dry quote needs no
// more, and a live quote issues an address that is never funded.
const refundTo = 't1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs';
const recipient = '0x1111111111111111111111111111111111111111';

class Answer {
  Answer(this.status, this.body);
  final int status;
  final String body;
}

final _http = HttpClient()..userAgent = 'splitz-contract-check';

Future<Answer> _send(String method, Uri url, [String? body]) async {
  final request = await _http.openUrl(method, url);
  if (body != null) {
    request.headers.contentType = ContentType.json;
    request.add(utf8.encode(body));
  }
  final response = await request.close();
  return Answer(
    response.statusCode,
    await response.transform(utf8.decoder).join(),
  );
}

/// The client, speaking through [answers] so every raw answer is kept, and
/// with `dry` rewritten when [dry] is set — the only field changed.
OneClickSwaps client(List<Answer> answers, {required bool dry}) =>
    OneClickSwaps(
      origin: Uri.parse(origin),
      zecAssetId: zecAssetId,
      deadline: () => protocol.canonicalInstant(
        DateTime.now()
            .toUtc()
            .add(const Duration(minutes: 10))
            .toIso8601String(),
      ),
      post: (url, body) async {
        final sent = jsonDecode(body) as Map<String, dynamic>;
        if (dry) sent['dry'] = true;
        final answer = await _send('POST', url, jsonEncode(sent));
        answers.add(answer);
        if (answer.status >= 400) {
          throw SwapException('answered ${answer.status}: ${answer.body}');
        }
        return answer.body;
      },
      get: (url) async {
        final answer = await _send('GET', url);
        answers.add(answer);
        if (answer.status >= 400) {
          throw SwapException('answered ${answer.status}: ${answer.body}');
        }
        return answer.body;
      },
    );

const usdc = TradableAsset(
  assetId: usdcOnBase,
  symbol: 'USDC',
  chain: 'base',
  decimals: 6,
);

SwapQuote unknownAddress() => SwapQuote(
  depositAddress: refundTo,
  amountInZatoshi: 1,
  amountOut: '1',
  asset: usdc,
  deadline: '2026-01-01T00:00:00.000Z',
);

Future<int> check() async {
  final failures = <String>[];
  final answers = <Answer>[];
  final swaps = client(answers, dry: true);

  final assets = await swaps.tradableAssets();
  final zec = assets.where((a) => a.assetId == zecAssetId).toList();
  if (zec.length != 1 || zec.single.chain != 'zec') {
    failures.add('the token list has no $zecAssetId on chain zec');
  }
  if (!assets.any((a) => a.assetId == usdcOnBase)) {
    failures.add('the token list has no $usdcOnBase');
  }
  print('tokens: ${assets.length}, ZEC on ${zec.map((a) => a.chain)}');

  // A dry quote issues no deposit address, so the client's reader, which
  // requires one, refuses it. The provider accepting the request is what is
  // checked here; the live-quote fixture pins the reader.
  try {
    await swaps.quote(
      asset: usdc,
      amountInZatoshi: 1000000,
      recipient: recipient,
      refundTo: refundTo,
    );
  } on SwapException catch (e) {
    final last = answers.last;
    if (last.status >= 400) failures.add('the quote was refused: $e');
  }
  final decoded = jsonDecode(answers.last.body);
  final quoted = decoded is Map ? decoded['quote'] : null;
  final amountOut = quoted is Map ? quoted['amountOut'] : null;
  print('dry quote: HTTP ${answers.last.status}, amountOut $amountOut');
  if (quoted is! Map) failures.add('the quote answer carries no quote');

  try {
    await swaps.statusOf(unknownAddress());
    failures.add('a status for an unknown address was answered as found');
  } on SwapException {
    if (answers.last.status != 404) {
      failures.add(
        'an unknown address answered ${answers.last.status}, '
        'not 404',
      );
    }
  }
  print('unknown status: HTTP ${answers.last.status}');

  for (final f in failures) {
    stderr.writeln('FAIL $f');
  }
  print(failures.isEmpty ? 'the live API accepts what the client sends' : '');
  return failures.isEmpty ? 0 : 1;
}

Future<int> capture(String dir) async {
  Directory(dir).createSync(recursive: true);
  void write(String name, Answer a) {
    File('$dir/$name').writeAsStringSync(
      '${const JsonEncoder.withIndent('  ').convert(jsonDecode(a.body))}\n',
    );
    print('$name: HTTP ${a.status}');
  }

  final answers = <Answer>[];
  final dry = client(answers, dry: true);
  await dry.tradableAssets();
  write('tokens.json', answers.last);
  try {
    await dry.quote(
      asset: usdc,
      amountInZatoshi: 1000000,
      recipient: recipient,
      refundTo: refundTo,
    );
  } on SwapException {
    // A dry quote has no deposit address; the answer is what is kept.
  }
  write('quote_dry.json', answers.last);

  final live = client(answers, dry: false);
  final issued = await live.quote(
    asset: usdc,
    amountInZatoshi: 1000000,
    recipient: recipient,
    refundTo: refundTo,
  );
  write('quote.json', answers.last);
  await live.statusOf(issued);
  write('status.json', answers.last);

  try {
    await live.statusOf(unknownAddress());
  } on SwapException {
    // Expected: the provider never issued this address.
  }
  write('status_unknown.json', answers.last);

  // The one hand-altered request: the client's own body without a required
  // field, to pin how the provider refuses.
  final body = <String, Object?>{
    'dry': true,
    'swapType': 'EXACT_INPUT',
    'originAsset': zecAssetId,
    'depositType': 'ORIGIN_CHAIN',
    'destinationAsset': usdcOnBase,
    'amount': '1000000',
    'refundTo': refundTo,
    'refundType': 'ORIGIN_CHAIN',
    'recipient': recipient,
    'recipientType': 'DESTINATION_CHAIN',
    'deadline': protocol.canonicalInstant(
      DateTime.now().toUtc().add(const Duration(minutes: 10)).toIso8601String(),
    ),
    'depositMode': 'SIMPLE',
  };
  write(
    'quote_refused.json',
    await _send('POST', Uri.parse('$origin/v0/quote'), jsonEncode(body)),
  );
  return 0;
}

Future<void> main(List<String> args) async {
  final code = switch (args) {
    ['check'] => await check(),
    ['capture', final dir] => await capture(dir),
    _ => () {
      stderr.writeln('usage: oneclick_live.dart check | capture <dir>');
      return 2;
    }(),
  };
  _http.close(force: true);
  exit(code);
}
