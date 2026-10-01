// The bill relay (SPEC.md §15.5) as a Cloudflare Worker, for an origin that
// stays up when no laptop does.
//
//     POST /c/<channel>   {"blobs": [...]}  -> {"ok": true}
//     GET  /c/<channel>                     -> {"blobs": [...]}
//
// The same two routes, answers and bounds as tools/relay/server.py: a channel
// is 64 lower-case hex digits, a blob at most MAX_BLOB_CHARS, a body at most
// MAX_BODY_BYTES, everything held at most MAX_HELD_CHARS. A push of a blob the
// channel already holds adds nothing; fetch answers every blob in the order it
// first arrived. It stores ciphertext under channel digests and can read none
// of it; nothing here logs a blob.
//
// Every channel lives in one Durable Object's SQLite store, so a push is
// checked against the whole store's bound and applied whole or not at all.
//
// A public origin takes pushes from anyone, so two bounds keep one sender
// from filling it for everybody:
//   - one client address adds at most MAX_SOURCE_CHARS in a UTC day; past it
//     the push is 429;
//   - a channel nobody has pushed to for EXPIRE_DAYS is dropped, so what a
//     stranger wrote does not stay once they stop.
// There is no bound per channel: anybody holding an invite knows its channel,
// and one that filled it would stop every other participant's pushes.
// What these turn away: several devices behind one address adding more than a
// day's bound between them, and a bill nobody has written to in EXPIRE_DAYS,
// which a device that still holds it pushes again on its next sync. Expiry
// runs before a fetch is answered and before a push is compared with what the
// channel holds, so that push stores everything it carries.
//
// An IPv6 sender is counted by its /64: one connection is handed a whole /64,
// so counting single addresses would hand each sender 2^64 bounds. An address
// is held only as SHA-256 of a random salt kept for that UTC day and the
// address. A day's salt is deleted with that day's counts, after which
// nothing here maps a count back to an address.

import { DurableObject } from "cloudflare:workers";

const MAX_BLOB_CHARS = 64 * 1024;
const MAX_BODY_BYTES = 32 * 1024 * 1024;
const CHANNEL = /^[0-9a-f]{64}$/;
const DAY_MS = 24 * 60 * 60 * 1000;

// The part of an address one sender controls: an IPv4 address whole, an IPv6
// address's first 64 bits.
export const sourceOf = (address) => {
  if (!address.includes(":")) return address;
  const [head, tail = ""] = address.toLowerCase().split("::");
  const left = head ? head.split(":") : [];
  const right = tail ? tail.split(":") : [];
  const groups = address.includes("::")
    ? [...left, ...Array(Math.max(0, 8 - left.length - right.length)).fill("0"), ...right]
    : left;
  return `${groups.slice(0, 4).map((g) => g.replace(/^0+(?=.)/, "")).join(":")}::/64`;
};

const hex = (bytes) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");

const answer = (status, payload) =>
  new Response(JSON.stringify(payload), {
    status,
    headers: { "Content-Type": "application/json" },
  });

export default {
  async fetch(request, env) {
    const parts = new URL(request.url).pathname.replace(/^\/+|\/+$/g, "").split("/");
    if (parts.length !== 2 || parts[0] !== "c" || !CHANNEL.test(parts[1])) {
      return answer(404, { error: "not a channel" });
    }
    if (request.method !== "GET" && request.method !== "POST") {
      return answer(405, { error: "GET or POST" });
    }
    const declared = Number(request.headers.get("Content-Length") ?? "0");
    if (declared > MAX_BODY_BYTES) {
      return answer(413, { error: `a body over ${MAX_BODY_BYTES} bytes` });
    }
    const store = env.RELAY.get(env.RELAY.idFromName("relay"));
    const source = request.headers.get("CF-Connecting-IP") ?? "";
    return store.handle(
      request.method,
      parts[1],
      request.method === "POST" ? await request.text() : "",
      source,
      Date.now(),
    );
  },
};

export class Relay extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.maxHeld = Number(env.MAX_HELD_CHARS ?? 512 * 1024 * 1024);
    this.maxSource = Number(env.MAX_SOURCE_CHARS ?? 64 * 1024 * 1024);
    this.expireMs = Number(env.EXPIRE_DAYS ?? 180) * DAY_MS;
    this.sql.exec(
      "CREATE TABLE IF NOT EXISTS blobs (" +
        "seq INTEGER PRIMARY KEY AUTOINCREMENT, channel TEXT NOT NULL, blob TEXT NOT NULL, " +
        "UNIQUE (channel, blob))",
    );
    this.sql.exec("CREATE TABLE IF NOT EXISTS held (id INTEGER PRIMARY KEY CHECK (id = 0), chars INTEGER NOT NULL)");
    this.sql.exec("INSERT OR IGNORE INTO held (id, chars) VALUES (0, 0)");
    this.sql.exec(
      "CREATE TABLE IF NOT EXISTS channels (channel TEXT PRIMARY KEY, chars INTEGER NOT NULL, last INTEGER NOT NULL)",
    );
    this.sql.exec("CREATE INDEX IF NOT EXISTS channels_last ON channels (last)");
    // `sources` held raw addresses; its replacement holds salted digests.
    this.sql.exec("DROP TABLE IF EXISTS sources");
    this.sql.exec(
      "CREATE TABLE IF NOT EXISTS source_days (source TEXT NOT NULL, day INTEGER NOT NULL, chars INTEGER NOT NULL, " +
        "PRIMARY KEY (source, day))",
    );
    this.sql.exec("CREATE TABLE IF NOT EXISTS salts (day INTEGER PRIMARY KEY, salt TEXT NOT NULL)");
    // Blobs held before `channels` existed are counted into it, stamped as
    // written now, so the expiry clock starts rather than dropping them.
    if (this.sql.exec("SELECT COUNT(*) AS n FROM channels").one().n === 0) {
      this.sql.exec(
        "INSERT INTO channels (channel, chars, last) " +
          "SELECT channel, SUM(LENGTH(blob)), ? FROM blobs GROUP BY channel",
        Date.now(),
      );
    }
  }

  // Drops every channel last pushed to before `now - expireMs`, and every
  // day's count and salt older than yesterday.
  expire(now) {
    const cutoff = now - this.expireMs;
    const stale = this.sql.exec("SELECT channel, chars FROM channels WHERE last < ?", cutoff).toArray();
    if (stale.length > 0) {
      this.ctx.storage.transactionSync(() => {
        for (const { channel, chars } of stale) {
          this.sql.exec("DELETE FROM blobs WHERE channel = ?", channel);
          this.sql.exec("DELETE FROM channels WHERE channel = ?", channel);
          this.sql.exec("UPDATE held SET chars = chars - ? WHERE id = 0", chars);
        }
      });
    }
    const yesterday = Math.floor(now / DAY_MS) - 1;
    this.sql.exec("DELETE FROM source_days WHERE day < ?", yesterday);
    this.sql.exec("DELETE FROM salts WHERE day < ?", yesterday);
  }

  // What `source` is counted under on `day`: a digest of that day's salt and
  // the address, never the address.
  async sourceKey(source, day) {
    let row = this.sql.exec("SELECT salt FROM salts WHERE day = ?", day).toArray()[0];
    if (row === undefined) {
      this.sql.exec(
        "INSERT OR IGNORE INTO salts (day, salt) VALUES (?, ?)",
        day,
        hex(crypto.getRandomValues(new Uint8Array(32))),
      );
      row = this.sql.exec("SELECT salt FROM salts WHERE day = ?", day).one();
    }
    return hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`${row.salt}|${sourceOf(source)}`)));
  }

  async handle(method, channel, raw, source = "", now = Date.now()) {
    if (method === "GET") {
      this.expire(now);
      const rows = this.sql.exec("SELECT blob FROM blobs WHERE channel = ? ORDER BY seq", channel).toArray();
      return answer(200, { blobs: rows.map((r) => r.blob) });
    }
    if (new TextEncoder().encode(raw).length > MAX_BODY_BYTES) {
      return answer(413, { error: `a body over ${MAX_BODY_BYTES} bytes` });
    }
    let blobs;
    try {
      blobs = JSON.parse(raw || "{}").blobs;
      if (!Array.isArray(blobs) || blobs.some((b) => typeof b !== "string")) throw new Error();
    } catch {
      return answer(400, { error: "not a push" });
    }
    if (blobs.some((b) => b.length > MAX_BLOB_CHARS)) {
      return answer(413, { error: `a blob over ${MAX_BLOB_CHARS} characters` });
    }
    // Taken before anything is read: the digest is awaited, and every step
    // after it runs without yielding, so no other push lands in between.
    const day = Math.floor(now / DAY_MS);
    const counted = await this.sourceKey(source, day);
    this.expire(now);
    // A repeat is the ordinary case, not an error: sync is idempotent, and a
    // relay that grew on every retry would punish a flaky connection.
    const seen = new Set(
      this.sql.exec("SELECT blob FROM blobs WHERE channel = ?", channel).toArray().map((r) => r.blob),
    );
    const fresh = [];
    for (const b of blobs) {
      if (!seen.has(b)) {
        seen.add(b);
        fresh.push(b);
      }
    }
    const grow = fresh.reduce((n, b) => n + b.length, 0);
    if (grow === 0) return answer(200, { ok: true });
    const held = this.sql.exec("SELECT chars FROM held WHERE id = 0").one().chars;
    if (held + grow > this.maxHeld) {
      return answer(507, { error: "the relay is full" });
    }
    const sent =
      this.sql.exec("SELECT chars FROM source_days WHERE source = ? AND day = ?", counted, day).toArray()[0]?.chars ?? 0;
    if (sent + grow > this.maxSource) {
      return answer(429, { error: "this address has pushed its bound for today" });
    }
    this.ctx.storage.transactionSync(() => {
      for (const b of fresh) this.sql.exec("INSERT INTO blobs (channel, blob) VALUES (?, ?)", channel, b);
      this.sql.exec("UPDATE held SET chars = chars + ? WHERE id = 0", grow);
      this.sql.exec(
        "INSERT INTO channels (channel, chars, last) VALUES (?, ?, ?) " +
          "ON CONFLICT (channel) DO UPDATE SET chars = chars + excluded.chars, last = excluded.last",
        channel,
        grow,
        now,
      );
      this.sql.exec(
        "INSERT INTO source_days (source, day, chars) VALUES (?, ?, ?) " +
          "ON CONFLICT (source, day) DO UPDATE SET chars = chars + excluded.chars",
        counted,
        day,
        grow,
      );
    });
    return answer(200, { ok: true });
  }
}
