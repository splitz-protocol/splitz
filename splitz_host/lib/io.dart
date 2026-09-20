/// Implementations that need a filesystem.
///
/// A separate entry point, so the rest of the package stays free of `dart:io`
/// and can be compiled for a target that has no files. A wallet imports this
/// one as well; nothing else needs to.
library;

export 'src/file_storage.dart';
