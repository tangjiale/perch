// 默认菜单随系统语言变化；统一屏蔽，由业务组件提供中文图片菜单。
// 仅阻止默认行为，不中断事件传播，保留文字选择和复制粘贴快捷键。
window.addEventListener("contextmenu", (event) => event.preventDefault(), true);
