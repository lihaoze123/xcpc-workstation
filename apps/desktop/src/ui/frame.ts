// Frame scheduling for splitting one interaction into two commits: the cheap
// visual acknowledgement commits with the input, the expensive content swap
// runs only after that frame has been presented (design-system §8.1 rule 2).

/**
 * Runs `fn` once the frame after the current one has started (double rAF), so
 * whatever the caller committed before reaches the screen first. Falls back to
 * a timer on hosts without a running frame clock (headless tests).
 */
export function afterNextPaint(fn: () => void, fallbackMs = 100): void {
  let done = false;
  const run = () => {
    if (done) return;
    done = true;
    clearTimeout(timer);
    fn();
  };
  const timer = setTimeout(run, fallbackMs);
  if (typeof requestAnimationFrame === "function") {
    requestAnimationFrame(() => requestAnimationFrame(run));
  }
}
