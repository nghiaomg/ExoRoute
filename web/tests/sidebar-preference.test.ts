import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { getStoredSidebarCollapsed, saveSidebarCollapsed } from '../src/lib/sidebar';

function withLocalStorage<T>(storage: Storage | undefined, run: () => T): T {
  const original = globalThis.localStorage;
  Object.defineProperty(globalThis, 'localStorage', {
    value: storage,
    configurable: true,
    writable: true,
  });
  try {
    return run();
  } finally {
    Object.defineProperty(globalThis, 'localStorage', {
      value: original,
      configurable: true,
      writable: true,
    });
  }
}

function memoryStorage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    clear: () => entries.clear(),
    getItem: (key) => entries.get(key) ?? null,
    key: (index) => [...entries.keys()][index] ?? null,
    removeItem: (key) => {
      entries.delete(key);
    },
    setItem: (key, value) => {
      entries.set(key, value);
    },
  };
}

test('the sidebar starts expanded when nothing was stored', () => {
  withLocalStorage(memoryStorage(), () => {
    assert.equal(getStoredSidebarCollapsed(), false);
  });
});

test('the collapsed preference survives a reload', () => {
  withLocalStorage(memoryStorage(), () => {
    saveSidebarCollapsed(true);
    assert.equal(getStoredSidebarCollapsed(), true);
    assert.equal(globalThis.localStorage.getItem('exoroute.sidebar-collapsed'), 'true');

    saveSidebarCollapsed(false);
    assert.equal(getStoredSidebarCollapsed(), false);
    assert.equal(globalThis.localStorage.getItem('exoroute.sidebar-collapsed'), 'false');
  });
});

test('an unreadable stored value falls back to expanded', () => {
  withLocalStorage(memoryStorage(), () => {
    globalThis.localStorage.setItem('exoroute.sidebar-collapsed', 'yes');
    assert.equal(getStoredSidebarCollapsed(), false);
  });
});

test('unavailable storage never breaks the shell', () => {
  withLocalStorage(undefined, () => {
    assert.equal(getStoredSidebarCollapsed(), false);
    assert.doesNotThrow(() => saveSidebarCollapsed(true));
  });

  withLocalStorage(
    {
      get length() {
        return 0;
      },
      clear: () => {},
      getItem: () => {
        throw new Error('storage disabled');
      },
      key: () => null,
      removeItem: () => {},
      setItem: () => {
        throw new Error('storage disabled');
      },
    },
    () => {
      assert.equal(getStoredSidebarCollapsed(), false);
      assert.doesNotThrow(() => saveSidebarCollapsed(true));
    },
  );
});
