/** Compare membership independently of order and repeated input values. */
export function sameMembers<T>(left: Iterable<T>, right: Iterable<T>): boolean {
  const leftSet = new Set(left);
  const rightSet = new Set(right);
  return leftSet.size === rightSet.size && [...leftSet].every(value => rightSet.has(value));
}
