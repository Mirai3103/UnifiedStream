import "@testing-library/jest-dom/vitest";

// Newer Node releases define a global `localStorage` that is unusable without `--localstorage-file`
// and shadows jsdom's. Install an in-memory stand-in so the interface's own storage handling is
// what the tests exercise, on any Node version. It is a real `Storage` subclass so tests can spy
// on `Storage.prototype` to simulate a blocked store.
if (typeof window.localStorage?.getItem !== "function") {
  class MemoryStorage implements Storage {
    #items = new Map<string, string>();
    get length() {
      return this.#items.size;
    }
    clear() {
      this.#items.clear();
    }
    getItem(key: string) {
      return this.#items.get(key) ?? null;
    }
    key(index: number) {
      return [...this.#items.keys()][index] ?? null;
    }
    removeItem(key: string) {
      this.#items.delete(key);
    }
    setItem(key: string, value: string) {
      this.#items.set(key, String(value));
    }
  }
  Object.defineProperty(window, "Storage", { value: MemoryStorage, configurable: true });
  Object.defineProperty(window, "localStorage", { value: new MemoryStorage(), configurable: true });
}
