import DOMPurify from "dompurify";

export function taskRichHtml(value: string): string {
  const html = /<\/?[a-z][a-z0-9]*(?:\s[^<>]*)?\/?>/i.test(value)
    ? value
    : value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").split(/\r?\n/).map(line => `<p>${line || "<br>"}</p>`).join("");
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: ["p", "br", "div", "span", "strong", "b", "em", "i", "u", "s", "strike", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "pre", "code", "hr", "a", "img", "table", "thead", "tbody", "tr", "th", "td"],
    ALLOWED_ATTR: ["src", "alt", "title", "href", "colspan", "rowspan", "width", "height", "start"],
    ALLOW_DATA_ATTR: false,
  });
}

export function taskNotesText(value: string): string {
  const doc = new DOMParser().parseFromString(taskRichHtml(value), "text/html");
  const text = doc.body.textContent?.trim() ?? "";
  return text || (doc.querySelector("img") ? "[图片]" : "暂无备注");
}
