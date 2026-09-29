// A SPEC.md §15.5 relay over HTTP: `POST <origin>/c/<channel>` with
// `{"blobs":[…]}` adds blobs, `GET <origin>/c/<channel>` returns every blob
// the channel holds.
//
// The library decides what goes on the wire and what an answer means
// (`relay_channel_url`, `relay_push_body`, `relay_push_answer`,
// `relay_fetch_answer`); this class moves the bytes with the `fetch` it is
// given, the global one of Node 18 and later by default. An answer is read
// whatever its HTTP status: the body says whether the relay took a push.
//
// Every failure throws `SplitzErrorHost`, whose `transient` says whether
// retrying later could succeed: a relay that could not be reached, or that
// refused or answered with something that is not a channel, is transient; an
// origin carrying a query or a fragment, or a blob over 65536 characters, is
// not.
import {
  SplitzErrorHost,
  relay_channel_url,
  relay_fetch_answer,
  relay_push_answer,
  relay_push_body,
} from "./splitz_ffi.js";

export class SplitzRelay {
  /// `origin` is a scheme, a host and an optional path; the channel is
  /// appended to it. `options.fetch` replaces the global `fetch`, so a wallet
  /// that routes its traffic sends bill sync the same way.
  constructor(origin, options = {}) {
    relay_channel_url(origin, "");
    this.origin = origin;
    this._fetch = options.fetch ?? globalThis.fetch;
    if (typeof this._fetch !== "function") {
      throw new SplitzErrorHost("This runtime has no fetch: pass options.fetch", false);
    }
  }

  /// Adds `blobs` to `channel`. Pushing a blob the channel already holds
  /// changes nothing, so a retry cannot create a duplicate. An empty list
  /// makes no request.
  async push(channel, blobs) {
    const body = relay_push_body(blobs);
    if (body === undefined) return;
    const url = relay_channel_url(this.origin, channel);
    relay_push_answer(await this._exchange("POST", url, body));
  }

  /// Every blob `channel` currently holds, including ones the caller already
  /// has; merging them is idempotent.
  async fetch(channel) {
    const url = relay_channel_url(this.origin, channel);
    return relay_fetch_answer(await this._exchange("GET", url, undefined));
  }

  async _exchange(method, url, body) {
    try {
      const response = await this._fetch(url, {
        method,
        body,
        headers: body === undefined ? {} : { "Content-Type": "application/json; charset=utf-8" },
      });
      return await response.text();
    } catch (e) {
      throw new SplitzErrorHost(`Could not reach the relay: ${e?.cause ?? e}`, true);
    }
  }
}
