import Foundation

/// A SPEC.md §15.5 relay over HTTP: `POST <origin>/c/<channel>` with
/// `{"blobs":[…]}` adds blobs, `GET <origin>/c/<channel>` returns every blob
/// the channel holds.
///
/// The library decides what goes on the wire and what an answer means
/// (`relayChannelUrl`, `relayPushBody`, `relayPushAnswer`,
/// `relayFetchAnswer`); this class moves the bytes with the `URLSession` it is
/// given. An answer is read whatever its HTTP status: the body says whether
/// the relay took a push.
///
/// Every failure throws `SplitzError.Host`, whose `transient` says whether
/// retrying later could succeed: a relay that could not be reached, or that
/// refused or answered with something that is not a channel, is transient; an
/// origin carrying a query or a fragment, or a blob over 65536 characters, is
/// not.
public final class SplitzRelay {
    /// A scheme, a host and an optional path. The channel is appended to it.
    public let origin: String
    private let session: URLSession

    public init(origin: String, session: URLSession = .shared) throws {
        _ = try relayChannelUrl(origin: origin, channel: "")
        self.origin = origin
        self.session = session
    }

    /// Adds `blobs` to `channel`. Pushing a blob the channel already holds
    /// changes nothing, so a retry cannot create a duplicate. An empty list
    /// makes no request.
    public func push(channel: String, blobs: [String]) async throws {
        guard let body = try relayPushBody(blobs: blobs) else { return }
        let url = try relayChannelUrl(origin: origin, channel: channel)
        try relayPushAnswer(body: try await exchange(method: "POST", url: url, body: body))
    }

    /// Every blob `channel` currently holds, including ones the caller
    /// already has; merging them is idempotent.
    public func fetch(channel: String) async throws -> [String] {
        let url = try relayChannelUrl(origin: origin, channel: channel)
        return try relayFetchAnswer(body: try await exchange(method: "GET", url: url, body: nil))
    }

    private func exchange(method: String, url: String, body: String?) async throws -> String {
        guard let address = URL(string: url) else {
            throw SplitzError.Host(
                detail: "Could not reach the relay: \(url) is not a URL", transient: true)
        }
        var request = URLRequest(url: address)
        request.httpMethod = method
        if let body = body {
            request.httpBody = Data(body.utf8)
            request.setValue("application/json; charset=utf-8", forHTTPHeaderField: "Content-Type")
        }
        let session = self.session
        let data: Data = try await withCheckedThrowingContinuation { continuation in
            session.dataTask(with: request) { data, _, error in
                if let error = error {
                    continuation.resume(throwing: SplitzError.Host(
                        detail: "Could not reach the relay: \(error.localizedDescription)",
                        transient: true))
                } else {
                    continuation.resume(returning: data ?? Data())
                }
            }.resume()
        }
        return String(decoding: data, as: UTF8.self)
    }
}
