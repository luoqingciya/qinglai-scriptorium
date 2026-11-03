import { describe, it, expect, beforeEach, vi } from "vitest";

import {
  readTheme,
  writeTheme,
  nextTheme,
  themeLabel,
  type Theme,
} from "./theme";

function fakeLocalStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: (_i: number) => null,
    length: 0,
  };
}

describe("theme persistence", () => {
  beforeEach(() => {
    vi.stubGlobal("localStorage", fakeLocalStorage());
  });

  it("readTheme 默认返回 dark", () => {
    expect(readTheme()).toBe("dark");
  });

  it("writeTheme → readTheme 往返一致", () => {
    const themes: Theme[] = ["dark", "light", "sepia"];
    for (const t of themes) {
      writeTheme(t);
      expect(readTheme()).toBe(t);
    }
  });

  it("localStorage 损坏时静默降级到 dark", () => {
    localStorage.setItem("qinglai.theme.v1", "garbage");
    expect(readTheme()).toBe("dark");
  });

  it("localStorage 不可用时静默降级", () => {
    const original = globalThis.localStorage;
    // 模拟 localStorage 完全不可用（隐私模式极端情况）
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("not available");
      },
      setItem: () => {
        throw new Error("not available");
      },
      removeItem: () => {},
      clear: () => {},
      key: () => null,
      length: 0,
    });

    expect(readTheme()).toBe("dark");
    // writeTheme 不抛异常
    expect(() => writeTheme("light")).not.toThrow();

    vi.stubGlobal("localStorage", original);
  });
});

describe("nextTheme cycle", () => {
  it("dark → light → sepia → dark 循环", () => {
    expect(nextTheme("dark")).toBe("light");
    expect(nextTheme("light")).toBe("sepia");
    expect(nextTheme("sepia")).toBe("dark");
  });
});

describe("themeLabel", () => {
  it("每个主题都有中文标签", () => {
    expect(themeLabel("dark")).toBe("暗色");
    expect(themeLabel("light")).toBe("亮色");
    expect(themeLabel("sepia")).toBe("护眼");
  });
});
