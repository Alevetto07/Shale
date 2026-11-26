class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = ResizeObserverMock as unknown as typeof ResizeObserver;
}

if (!HTMLElement.prototype.hasPointerCapture) {
  HTMLElement.prototype.hasPointerCapture = () => false;
}
if (!HTMLElement.prototype.setPointerCapture) {
  HTMLElement.prototype.setPointerCapture = () => undefined;
}
if (!HTMLElement.prototype.releasePointerCapture) {
  HTMLElement.prototype.releasePointerCapture = () => undefined;
}
if (!HTMLElement.prototype.scrollIntoView) {
  HTMLElement.prototype.scrollIntoView = () => undefined;
}

// number-flow calls willUpdate on its custom element. jsdom does not
// upgrade that element, so the method is missing and React unmounts the tree.
const elementProto = HTMLElement.prototype as HTMLElement & {
  willUpdate?: () => void;
  didUpdate?: () => void;
};
if (typeof elementProto.willUpdate !== "function") {
  elementProto.willUpdate = () => undefined;
}
if (typeof elementProto.didUpdate !== "function") {
  elementProto.didUpdate = () => undefined;
}
