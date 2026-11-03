let n = 0;

export function bump(): number {
  n += 1;
  return n;
}

export function count(): number {
  return n;
}
