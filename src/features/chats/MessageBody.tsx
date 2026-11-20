import Markdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import { memo, useEffect, useState, type ComponentProps } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createPortal } from "react-dom";
import Dialog from "../../components/Dialog";

const localImage = /^ai-gent-image:([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/;

function PreviewImage({ src, alt, ...props }: ComponentProps<"img">) {
  const [open, setOpen] = useState(false);
  return <><button type="button" className="message-image-thumbnail" aria-label={`放大图片：${alt || "图片"}`} aria-haspopup="dialog" onClick={event => { event.preventDefault(); event.stopPropagation(); setOpen(true); }}>
    <img {...props} src={src} alt={alt} decoding="async" />
  </button>{open && createPortal(<Dialog title="图片预览" onClose={() => setOpen(false)}><div className="image-preview"><img src={src} alt={alt} decoding="async" /></div></Dialog>, document.body)}</>;
}

function LocalImage({ id, alt }: { id: string; alt: string }) {
  const [image, setImage] = useState<{ id: string; url?: string; error?: string }>();
  useEffect(() => {
    let active = true;
    invoke<string>("image_read", { id }).then(
      url => { if (active) setImage({ id, url }); },
      () => { if (active) setImage({ id, error: "图片读取失败" }); },
    );
    return () => { active = false; };
  }, [id]);
  return image?.id === id && image.url
    ? <PreviewImage src={image.url} alt={alt} />
    : <span role="status">{image?.id === id && image.error ? image.error : "正在读取图片…"}</span>;
}

const components: NonNullable<ComponentProps<typeof Markdown>["components"]> = {
  table: ({ children }) => <div className="message-table" role="region" aria-label="消息表格" tabIndex={0}><table>{children}</table></div>,
  a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" />,
  img: ({ node: _node, src, alt, ...props }) => {
    const id = src?.match(localImage)?.[1];
    return id ? <LocalImage id={id} alt={alt ?? "生成的图片"} /> : src ? <PreviewImage {...props} src={src} alt={alt} /> : <span>{alt ?? "图片不可用"}</span>;
  },
};

export default memo(function MessageBody({ text }: { text: string }) {
  return <div className="message-body"><Markdown remarkPlugins={[remarkGfm]} urlTransform={(url, key) =>
    key === "src" && (localImage.test(url) || /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(url)) ? url : defaultUrlTransform(url)
  } components={components}>{text}</Markdown></div>;
});
