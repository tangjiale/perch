// 仅由原生 WebView 注入；保留应用按钮、文字选择和复制粘贴快捷键。
window.addEventListener("contextmenu", (event) => event.preventDefault(), true);
