import { useEffect, useState } from "react";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import DOMPurify from "dompurify";
import {
  Bold,
  Italic,
  List,
  ListOrdered,
  Redo2,
  Underline,
  Undo2,
} from "lucide-react";

const allowedTags = [
  "p", "br", "strong", "b", "em", "i", "u", "s", "del", "ul", "ol",
  "li", "blockquote", "pre", "code", "h1", "h2", "h3",
];

function cleanMailHtml(html: string) {
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: allowedTags,
    ALLOWED_ATTR: [],
  });
}

function textAsHtml(text: string) {
  return text
    .split(/\r\n?|\n/)
    .map((line) => `<p>${line ? line.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;") : "<br>"}</p>`)
    .join("");
}

export default function MailRichTextEditor({
  text,
  html,
  disabled,
  onChange,
}: {
  text: string;
  html?: string;
  disabled: boolean;
  onChange: (value: { text: string; html: string }) => void;
}) {
  const [initialContent] = useState(() =>
    html ? cleanMailHtml(html) : textAsHtml(text),
  );
  const editor = useEditor({
    extensions: [StarterKit],
    content: initialContent,
    editable: !disabled,
    editorProps: {
      attributes: {
        role: "textbox",
        "aria-label": "邮件正文富文本",
        "aria-multiline": "true",
      },
      transformPastedHTML: cleanMailHtml,
    },
    onUpdate: ({ editor: current }) => {
      onChange({
        text: current.getText({ blockSeparator: "\n" }),
        html: cleanMailHtml(current.getHTML()),
      });
    },
  });

  useEffect(() => {
    editor?.setEditable(!disabled, false);
  }, [disabled, editor]);

  const actions = [
    {
      label: "粗体",
      icon: Bold,
      active: editor?.isActive("bold") ?? false,
      run: () => editor?.chain().focus().toggleBold().run(),
    },
    {
      label: "斜体",
      icon: Italic,
      active: editor?.isActive("italic") ?? false,
      run: () => editor?.chain().focus().toggleItalic().run(),
    },
    {
      label: "下划线",
      icon: Underline,
      active: editor?.isActive("underline") ?? false,
      run: () => editor?.chain().focus().toggleUnderline().run(),
    },
    {
      label: "项目符号列表",
      icon: List,
      active: editor?.isActive("bulletList") ?? false,
      run: () => editor?.chain().focus().toggleBulletList().run(),
    },
    {
      label: "编号列表",
      icon: ListOrdered,
      active: editor?.isActive("orderedList") ?? false,
      run: () => editor?.chain().focus().toggleOrderedList().run(),
    },
    {
      label: "撤销",
      icon: Undo2,
      active: false,
      run: () => editor?.chain().focus().undo().run(),
    },
    {
      label: "重做",
      icon: Redo2,
      active: false,
      run: () => editor?.chain().focus().redo().run(),
    },
  ];

  return (
    <div className="mail-rich-editor">
      <div className="mail-rich-toolbar" role="toolbar" aria-label="邮件正文格式">
        {actions.map(({ label, icon: Icon, active, run }) => (
          <button
            key={label}
            type="button"
            disabled={disabled || !editor}
            aria-label={label}
            aria-pressed={active}
            title={label}
            onMouseDown={(event) => event.preventDefault()}
            onClick={run}
          >
            <Icon size={16} aria-hidden="true" />
          </button>
        ))}
      </div>
      <EditorContent editor={editor} className="mail-rich-content" />
    </div>
  );
}
