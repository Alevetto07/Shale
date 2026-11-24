/**
 * Motion springs — apple-design defaults: critically damped (bounce 0),
 * slight bounce only on gesture release paths (see springGestureRelease).
 */
export const springSnappy = { type: "spring" as const, duration: 0.3, bounce: 0 };
export const springTab = { type: "spring" as const, duration: 0.4, bounce: 0 };
export const springPanel = { type: "spring" as const, duration: 0.45, bounce: 0 };
/** After flick / drag release — light bounce only when momentum mattered. */
export const springGestureRelease = { type: "spring" as const, duration: 0.45, bounce: 0.12 };