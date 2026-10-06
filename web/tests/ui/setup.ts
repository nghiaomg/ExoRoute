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
// Ark's combobox looks the highlighted option up with CSS.escape, which jsdom
// does not implement. Escaping the reserved characters is enough for the
// data-value selectors the widgets build from provider and model ids.
if (typeof (globalThis as { CSS?: unknown }).CSS === 'undefined') {
  Object.defineProperty(globalThis, 'CSS', {
    value: {
      escape: (value: string) => String(value).replace(/[^a-zA-Z0-9_-]/g, (char) => `\\${char}`),
    },
  });
}
// Ark UI's popover positioning scrolls its content element, which jsdom does
// not implement.
Element.prototype.scrollTo = () => {};
