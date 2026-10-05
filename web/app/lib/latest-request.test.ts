import { describe, expect, it } from "vitest";
import { createLatestRequestGuard } from "./latest-request";

/** A promise whose resolution the test controls. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

describe("createLatestRequestGuard", () => {
  it("keeps the newest result when an older request finishes last", async () => {
    // Mirrors the Add Host form: a re-enroll token is requested, the form is
    // closed and reopened (which requests a normal token), and the first
    // response arrives after the second.
    const guard = createLatestRequestGuard();
    let shown: string | null = null;
    const issue = async (response: Promise<string>) => {
      const isLatest = guard.begin();
      const token = await response;
      if (isLatest()) shown = token;
    };

    const reenroll = deferred<string>();
    const normal = deferred<string>();
    const first = issue(reenroll.promise);
    const second = issue(normal.promise);

    normal.resolve("normal-token");
    await second;
    reenroll.resolve("reenroll-token");
    await first;

    expect(shown).toBe("normal-token");
  });

  it("applies a result that is still the latest", async () => {
    const guard = createLatestRequestGuard();
    const isLatest = guard.begin();
    await Promise.resolve();
    expect(isLatest()).toBe(true);
  });

  it("drops an in-flight result once invalidated", () => {
    const guard = createLatestRequestGuard();
    const isLatest = guard.begin();
    guard.invalidate();
    expect(isLatest()).toBe(false);
    // A later request is unaffected by the earlier invalidation.
    expect(guard.begin()()).toBe(true);
  });
});
