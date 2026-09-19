export function markRuns(
  text: string,
  marks: ReadonlyArray<readonly [number, number]>,
): Array<{ text: string; marked: boolean }> {
  const ranges = marks
    .map(([from, to]) => [Math.max(0, Math.trunc(from)), Math.min(text.length, Math.trunc(to))])
    .filter(([from, to]) => Number.isFinite(from) && Number.isFinite(to) && from < to)
    .sort((a, b) => a[0] - b[0]);
  const merged: Array<[number, number]> = [];
  for (const [from, to] of ranges) {
    const previous = merged[merged.length - 1];
    if (previous && from <= previous[1]) previous[1] = Math.max(previous[1], to);
    else merged.push([from, to]);
  }
  const runs: Array<{ text: string; marked: boolean }> = [];
  let cursor = 0;
  for (const [from, to] of merged) {
    if (cursor < from) runs.push({ text: text.slice(cursor, from), marked: false });
    runs.push({ text: text.slice(from, to), marked: true });
    cursor = to;
  }
  if (cursor < text.length) runs.push({ text: text.slice(cursor), marked: false });
  return runs;
}
