// The extension APIs with promises in every browser: Firefox's `browser.*`
// returns promises, its `chrome.*` takes callbacks only; Chrome's `chrome.*`
// returns promises. (Neither exists in unit tests.)
const g = globalThis as unknown as { browser?: typeof chrome; chrome?: typeof chrome };
export const ext = (g.browser ?? g.chrome) as typeof chrome;
