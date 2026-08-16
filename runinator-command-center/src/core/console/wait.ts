// waiting, the way a console command has to wait.

/// a sleep that gives up as soon as the command is stopped, so a stopped `watch` does not linger
/// for another interval before noticing.
export function delay(milliseconds: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(finish, milliseconds);

    function finish() {
      clearTimeout(timer);
      signal.removeEventListener("abort", finish);
      resolve();
    }

    signal.addEventListener("abort", finish, { once: true });
  });
}
