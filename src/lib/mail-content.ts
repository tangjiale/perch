import DOMPurify from "dompurify";

export function mailHtmlDocument(html: string, dark: boolean): string {
  // 邮件是外部内容：禁止脚本、表单、导航和远程资源，正文只在沙箱 iframe 内显示。
  const clean = DOMPurify.sanitize(html, {
    FORBID_TAGS: [
      "script",
      "style",
      "form",
      "input",
      "button",
      "textarea",
      "select",
      "iframe",
      "object",
      "embed",
      "base",
      "meta",
      "link",
      "svg",
      "math",
      "video",
      "audio",
      "img",
    ],
    FORBID_ATTR: [
      "href",
      "src",
      "srcset",
      "action",
      "formaction",
      "poster",
      "background",
    ],
  });
  return `<!doctype html><html><head><meta charset="UTF-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; form-action 'none'; base-uri 'none'"><meta name="color-scheme" content="${dark ? "dark" : "light"}"><style>html,*{overscroll-behavior:none}body{margin:20px;font:14px/1.75 -apple-system,BlinkMacSystemFont,sans-serif;overflow-wrap:anywhere;color:${dark ? "#ededf0" : "#29333c"};background:${dark ? "#282a2d" : "#fff"}}table{max-width:100%!important}pre{white-space:pre-wrap}*{max-width:100%;box-sizing:border-box}</style></head><body>${clean}</body></html>`;
}

export function mailReplyRecipients(
  to: string,
  cc: string,
  sender: string,
  own: string,
): string {
  const addresses =
    `${sender},${to},${cc}`.match(
      /[A-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Z0-9.-]+\.[A-Z]{2,}/gi,
    ) ?? [];
  const unique = new Map<string, string>();
  addresses.forEach((address) => {
    if (address.toLowerCase() !== own.toLowerCase())
      unique.set(address.toLowerCase(), address);
  });
  return [...unique.values()].join(", ");
}
