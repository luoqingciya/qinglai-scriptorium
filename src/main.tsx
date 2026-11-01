import { createRoot } from "react-dom/client";

import App from "./App";
import "./styles.css";

// 刻意不使用 StrictMode：它会在开发期双挂载 effect，
// 导致 Milkdown 编辑器被创建/销毁两次，输入法状态也会被打断。
const host = document.getElementById("root");
if (!host) throw new Error("找不到 #root 挂载点");

createRoot(host).render(<App />);
