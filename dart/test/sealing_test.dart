/// §11.3's producing half, against its own reader.
///
/// The cipher is the host's, so what is asserted here is every value the
/// specification fixes: the plaintext, the nonce derived from it, the frame,
/// and the channel. A wallet deriving any of these a second time is a second
/// place for them to drift, and two devices that drift produce blobs neither
/// can open.
library;

import 'dart:convert';

import 'package:test/test.dart';
import 'package:splitz_core/splitz_core.dart';

/// Stands in for the cipher's output: this library never produces one.
List<int> fakeBody(int length) => List<int>.generate(length, (i) => i % 251);

void main() {
  group('the plaintext', () {
    test('is canonical JSON, so two orderings seal identically', () {
      final one = <String, dynamic>{'b': 2, 'a': 1};
      final other = <String, dynamic>{'a': 1, 'b': 2};
      expect(sealedPlaintext(one), sealedPlaintext(other));
      expect(utf8.decode(sealedPlaintext(one)), canonicalJson(one));
    });
  });

  group('the nonce', () {
    test('is SHA-256 of the plaintext, truncated', () {
      final plaintext = utf8.encode('an entry');
      expect(sealedNonce(plaintext).length, nonceBytes);
      expect(sealedNonce(plaintext), sha256(plaintext).sublist(0, nonceBytes));
    });

    test('one entry always seals under one nonce', () {
      // Idempotence is what keeps a channel finite: a relay stores the blob
      // once however many times it is pushed.
      final entry = <String, dynamic>{'kind': 'joinBill', 'n': 1};
      expect(sealedNonce(sealedPlaintext(entry)),
          sealedNonce(sealedPlaintext(entry)));
    });

    test('one byte of difference gives a different nonce', () {
      // The one condition the cipher requires: two different plaintexts never
      // share a nonce.
      final a = sealedNonce(utf8.encode('an entry'));
      final b = sealedNonce(utf8.encode('an entrz'));
      expect(a, isNot(b));
    });
  });

  group('the frame', () {
    test('what is framed is what the reader reads back', () {
      final plaintext = sealedPlaintext(<String, dynamic>{'kind': 'joinBill'});
      final nonce = sealedNonce(plaintext);
      final body = fakeBody(tagBytes + 40);

      final framed = frameSealed(nonce, body);
      final read = parseSealedFrame(framed);

      expect(read.version, sealedVersion);
      expect(read.bodyBytes, body.length);
      // The nonce survives the round trip, which is what lets a reader
      // decrypt without a length field.
      expect(read.nonce, base64UrlEncode(nonce).replaceAll('=', ''));
    });

    test('a nonce of the wrong length is refused', () {
      expect(
        () => frameSealed(fakeBody(nonceBytes - 1), fakeBody(tagBytes)),
        throwsA(isA<SplitError>()
            .having((e) => e.code, 'code', SplitCode.sealedMalformed)),
      );
    });

    test('a body too short to hold a tag is refused', () {
      // The reader refuses such a frame, so producing one would emit a blob
      // nothing can open.
      expect(
        () => frameSealed(fakeBody(nonceBytes), fakeBody(tagBytes - 1)),
        throwsA(isA<SplitError>()
            .having((e) => e.code, 'code', SplitCode.sealedMalformed)),
      );
    });

    test('the shortest frame this writes is one the reader accepts', () {
      final framed = frameSealed(fakeBody(nonceBytes), fakeBody(tagBytes));
      expect(parseSealedFrame(framed).bodyBytes, tagBytes);
    });
  });

  group('the channel', () {
    test('is the bill id digest, not the bill id', () {
      // The id is a live address printed in every invite; a relay that only
      // ever sees traffic cannot run the digest backwards.
      const billId = 'HqA9d4fLlNHBmVZHGH3s6w';
      expect(channelFor(billId), sha256Hex(utf8.encode(billId)));
      expect(channelFor(billId), isNot(contains(billId)));
    });

    test('is lower-case hex, 64 characters', () {
      final channel = channelFor('HqA9d4fLlNHBmVZHGH3s6w');
      expect(channel.length, 64);
      expect(channel, matches(RegExp(r'^[0-9a-f]{64}$')));
    });

    test('every participant computes the same channel', () {
      expect(channelFor('bill-1'), channelFor('bill-1'));
      expect(channelFor('bill-1'), isNot(channelFor('bill-2')));
    });
  });
}
