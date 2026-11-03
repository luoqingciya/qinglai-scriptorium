// 通过 CDP 检查 Tauri 窗口的真实渲染结果。
//
// 背景：Tauri 的 debug 构建默认加载 devUrl（localhost:1420），直接跑 exe 而不起
// Vite，会得到一个「无法访问此页面」的错误页——而进程照样存活，只看进程状态
// 会误判为「启动成功」。截屏又被安全策略挡住。这个脚本走 WebView2 的远程调试
// 端口读 DOM，是唯一能真正确认界面的手段。
//
// 用法：
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" pnpm tauri dev
//   node scripts/ui-check.mjs                          # 只检查空状态界面
//   node scripts/ui-check.mjs --vault <绝对路径>        # 预置稿库后检查真实功能
//
// 退出码非 0 表示界面有问题（错误页、白屏、关键元素缺失、有 console 报错）。

const PORT = process.env.CDP_PORT || 9222;
const TIMEOUT_MS = 15000;

/** 预置稿库：写 localStorage 让应用启动时自动打开 */
const vaultArg = process.argv.indexOf("--vault");

/**
 * git bash 的 `$(pwd)` 给出 `/d/Project/...`，而 Rust 侧需要 `D:/Project/...`。
 * 这里把单个字母盘符的 POSIX 路径转回 Windows 形式，README 里的命令才可直接复制。
 */
function toWindowsPath(p) {
  const m = /^\/([a-zA-Z])\//.exec(p);
  return m ? `${m[1].toUpperCase()}:/${p.slice(3)}` : p;
}

const VAULT =
  vaultArg > -1 && process.argv[vaultArg + 1]
    ? toWindowsPath(process.argv[vaultArg + 1])
    : null;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function fetchTargets() {
  const res = await fetch(`http://127.0.0.1:${PORT}/json`);
  if (!res.ok) throw new Error(`CDP 不可达：HTTP ${res.status}`);
  return res.json();
}

/** 等一个可用的 page target，应用刚启动时可能还没注册 */
async function waitForPage(deadline) {
  while (Date.now() < deadline) {
    try {
      const targets = await fetchTargets();
      const page = targets.find((t) => t.type === "page" && t.webSocketDebuggerUrl);
      if (page) return page;
    } catch {
      // 端口还没起来，继续等
    }
    await sleep(500);
  }
  throw new Error("等待 WebView 页面超时");
}

function send(ws, id, method, params = {}) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${method} 超时`)), 10000);

    const onMessage = (event) => {
      const msg = JSON.parse(event.data);
      if (msg.id !== id) return;
      clearTimeout(timer);
      ws.removeEventListener("message", onMessage);
      if (msg.error) reject(new Error(JSON.stringify(msg.error)));
      else resolve(msg.result);
    };

    ws.addEventListener("message", onMessage);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(ws, id, expression) {
  const result = await send(ws, id, "Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  return result?.result?.value;
}

const PROBE = `(() => {
  const text = (document.body?.innerText ?? "").replace(/\\s+/g, " ").trim();
  const q = (sel) => !!document.querySelector(sel);
  const count = (sel) => document.querySelectorAll(sel).length;
  const editorText = document.querySelector(".ProseMirror")?.innerText ?? "";
  return {
    title: document.title,
    url: location.href,
    textLength: text.length,
    textHead: text.slice(0, 300),
    hasApp: q(".app"),
    hasExplorer: q(".explorer"),
    hasEditorHost: q(".editor-host"),
    hasProseMirror: q(".ProseMirror"),
    hasStatusBar: q(".statusbar"),
    chapterCount: count(".chapter-row"),
    characterTab: q(".explorer-tab"),
    editorHead: editorText.replace(/\\s+/g, " ").trim().slice(0, 80),
    // Tauri 内置错误页 / 浏览器错误页的特征
    looksLikeErrorPage: /ERR_CONNECTION|无法访问此页面|拒绝连接|refused to connect/i.test(text),
  };
})()`;

const main = async () => {
  if (VAULT) console.log("稿库：", VAULT);

  const deadline = Date.now() + TIMEOUT_MS;
  const page = await waitForPage(deadline);

  const ws = new WebSocket(page.webSocketDebuggerUrl);
  const consoleErrors = [];

  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("WebSocket 连接超时")), 8000);
    ws.addEventListener("open", () => {
      clearTimeout(timer);
      resolve();
    });
    ws.addEventListener("error", () => {
      clearTimeout(timer);
      reject(new Error("WebSocket 连接失败"));
    });
  });

  // 先订阅控制台与异常，再求值，才能收到页面自己的报错
  ws.send(JSON.stringify({ id: 1, method: "Runtime.enable" }));
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    if (msg.method === "Runtime.consoleAPICalled" && msg.params.type === "error") {
      consoleErrors.push(
        msg.params.args.map((a) => a.value ?? a.description ?? "").join(" "),
      );
    }
    if (msg.method === "Runtime.exceptionThrown") {
      const d = msg.params.exceptionDetails;
      consoleErrors.push(d.exception?.description ?? d.text ?? "未捕获异常");
    }
  });

  // 预置稿库：写 localStorage 后刷新，让应用走「恢复上次打开的库」这条真实路径
  if (VAULT) {
    await evaluate(
      ws,
      2,
      `localStorage.setItem("qinglai.vault.v1", ${JSON.stringify(VAULT)}); "ok"`,
    );
    await send(ws, 3, "Page.reload", { ignoreCache: true });
    // 等 React 重新挂载 + 扫描稿库 + 读第一篇文章
    await sleep(3500);
  }

  const info = await evaluate(ws, 4, PROBE);
  ws.close();

  console.log("页面：", info.url);
  console.log("标题：", info.title);
  console.log("文本长度：", info.textLength);
  console.log("正文开头：", info.textHead.slice(0, 140) || "(空)");
  console.log("元素：", {
    app: info.hasApp,
    explorer: info.hasExplorer,
    editorHost: info.hasEditorHost,
    proseMirror: info.hasProseMirror,
    statusBar: info.hasStatusBar,
  });
  if (VAULT) {
    console.log("章节数：", info.chapterCount);
    console.log("编辑器开头：", info.editorHead || "(空)");
  }
  if (consoleErrors.length > 0) {
    console.log("控制台报错：");
    for (const e of consoleErrors.slice(0, 10)) console.log("  -", e);
  } else {
    console.log("控制台报错：无");
  }

  const problems = [];
  if (info.looksLikeErrorPage) problems.push("页面是连接错误页（dev server 没起？）");
  if (!info.hasApp) problems.push("缺少 .app 根元素（React 未挂载？）");
  if (info.textLength < 10) problems.push("页面几乎无文本（白屏？）");
  if (consoleErrors.length > 0) problems.push(`有 ${consoleErrors.length} 条控制台报错`);
  if (VAULT) {
    if (info.chapterCount === 0) problems.push("稿库已打开但没有章节（扫描失败？）");
    if (!info.hasProseMirror) problems.push("编辑器未挂载");
    if (!info.editorHead) problems.push("编辑器内容为空（读取失败？）");
  }

  if (problems.length > 0) {
    console.error("\n✗ 界面检查未通过：");
    for (const p of problems) console.error("  -", p);
    process.exit(1);
  }

  console.log("\n✓ 界面检查通过");
};

main().catch((err) => {
  console.error("✗ 检查失败：", err.message);
  process.exit(1);
});
