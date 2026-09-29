/** A SPEC.md §15.5 relay over HTTP. Every failure throws `SplitzErrorHost`. */
export declare class SplitzRelay {
  /** A scheme, a host and an optional path; the channel is appended to it. */
  readonly origin: string;
  constructor(origin: string, options?: { fetch?: typeof globalThis.fetch });
  /** Adds `blobs` to `channel`; a blob already held changes nothing. */
  push(channel: string, blobs: string[]): Promise<void>;
  /** Every blob `channel` currently holds. */
  fetch(channel: string): Promise<string[]>;
}
