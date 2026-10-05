/**
 * Lets a component ignore async results that are no longer wanted.
 *
 * When the same request can be issued again before the previous one
 * settles (a "new token" button, a form that is closed and reopened), the
 * responses may arrive out of order. Applying a late one silently replaces
 * the state the user is looking at with an answer to an older question.
 *
 * `begin()` marks a new request as the current one and returns a predicate
 * that stays true only while it is still the latest. `invalidate()` drops
 * whatever is in flight without starting anything new.
 */
export interface LatestRequestGuard {
  begin(): () => boolean;
  invalidate(): void;
}

export function createLatestRequestGuard(): LatestRequestGuard {
  let current = 0;
  return {
    begin() {
      const ticket = ++current;
      return () => ticket === current;
    },
    invalidate() {
      current += 1;
    },
  };
}
