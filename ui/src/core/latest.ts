/**
 * Hands out a turn for each call. Only the newest turn is still current, so the answer of an older
 * request that comes back late can be dropped instead of replacing a newer one.
 */
export function newestOnly(): () => () => boolean {
  let latest = 0;
  return () => {
    const mine = ++latest;
    return () => mine === latest;
  };
}
