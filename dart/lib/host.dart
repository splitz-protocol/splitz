/// The wallet seam: the entries a log is made of, and what a wallet lends.
///
/// `package:splitz_core/splitz_core.dart` decides what a bill is, what anyone owes and
/// what payment request settles it. This entry point supplies the two things
/// §13 leaves to the wallet that embeds it: something that assembles an entry
/// and derives §9.5's id, and the seam through which a wallet lends its keys,
/// its clock and its ability to send.
///
/// Nothing behind this import holds a key, opens a socket or reads a clock of
/// its own, so the whole feature runs in a test with no wallet behind it.
///
/// Importing it does not pull in the protocol's own surface. A host needs
/// both:
///
/// ```dart
/// import 'package:splitz_core/splitz_core.dart' as splitz;
/// import 'package:splitz_core/host.dart';
/// ```
library;

/// The two answers §14 holds back, named where a caller meets them.
export 'splitz_core.dart'
    show
        Awaiting,
        Delta,
        DeltaSquare,
        Bill,
        NothingMissing,
        Participant,
        participantId,
        Payout,
        Settlement,
        TooBigForOneSquare;

export 'src/host/bill_log.dart';
export 'src/host/entries.dart';
export 'src/host/host.dart';
export 'src/host/lanes.dart';
export 'src/host/settle_flow.dart';
export 'src/host/sharing.dart';
