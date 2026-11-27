import { useEffect, useRef, type ReactNode } from "react";
import Icon from "./Icon";
export default function Dialog({ title, children, onClose, wide = false, className = "" }: { title: string; children: ReactNode; onClose: () => void; wide?: boolean; className?: string }) {
  const ref = useRef<HTMLDialogElement>(null);
  const returnFocus = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const close = useRef(onClose); close.current = onClose;
  useEffect(() => {
    const element = ref.current;
    element?.showModal();
    const target = element?.querySelector<HTMLElement>("[autofocus], input:not([type=hidden]), textarea, select") ?? element;
    target?.focus();
    return () => { element?.close(); returnFocus.current?.focus(); };
  }, []);
  return <dialog ref={ref} tabIndex={-1} className={`dialog ${wide ? "dialog-wide" : ""} ${className}`.trim()} aria-label={title} onCancel={event => { event.preventDefault(); close.current(); }} onClick={event => {
    if (event.target !== event.currentTarget) return;
    const rect = event.currentTarget.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) close.current();
  }}><div className="dialog-heading"><div><h2>{title}</h2></div><button className="icon-button" onClick={onClose} aria-label="关闭弹窗"><Icon name="close" /></button></div>{children}</dialog>;
}
