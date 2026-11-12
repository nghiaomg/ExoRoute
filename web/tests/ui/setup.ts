import { cleanup } from '@testing-library/svelte';
import { afterEach } from 'vitest';

afterEach(() => {
  cleanup();
});

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }),
});

class TestResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

class TestIntersectionObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

Object.defineProperty(window, 'ResizeObserver', { value: TestResizeObserver });
Object.defineProperty(window, 'IntersectionObserver', { value: TestIntersectionObserver });
Element.prototype.scrollIntoView = () => {};
// Dropdown panels keep the highlighted option in view with scrollIntoView and
// may scroll their content element, neither of which jsdom implements.
Element.prototype.scrollTo = () => {};
// jsdom implements dialog.close()/cancel-less semantics but not showModal(),
// which the native-dialog system (shell delegation + ArkDialog replacement)
// relies on; polyfill it as "mark open" — tests assert dialog[open].
if (typeof HTMLDialogElement !== 'undefined' && !HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement): void {
    this.setAttribute('open', '');
  };
}
