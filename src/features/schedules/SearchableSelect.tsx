import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import Icon from "../../components/Icon";

export interface SelectOption {
  value: string;
  label: string;
  subtitle?: string;
  badge?: string;
  leading?: ReactNode;
  keywords?: string;
}

export default function SearchableSelect({
  value,
  options,
  placeholder = "请选择…",
  searchPlaceholder = "输入关键词搜索…",
  searchable = true,
  compact = false,
  disabled = false,
  emptyText = "没有匹配项",
  onChange,
}: {
  value: string;
  options: SelectOption[];
  placeholder?: string;
  searchPlaceholder?: string;
  searchable?: boolean;
  compact?: boolean;
  disabled?: boolean;
  emptyText?: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const selected = useMemo(
    () => options.find(opt => opt.value === value),
    [options, value],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter(opt => {
      const hay = `${opt.label} ${opt.subtitle ?? ""} ${opt.keywords ?? ""} ${opt.value}`.toLowerCase();
      return hay.includes(q);
    });
  }, [options, query]);

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    const timer = window.setTimeout(() => {
      inputRef.current?.focus();
    }, 10);
    const onPointerDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div
      ref={rootRef}
      className={`combo-root ${compact ? "compact" : ""} ${open ? "is-open" : ""}`}
    >
      <button
        type="button"
        className="combo-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen(v => !v)}
      >
        <span className="combo-trigger-content">
          {selected?.leading && <span className="combo-leading">{selected.leading}</span>}
          <span className={`combo-trigger-label ${selected ? "" : "placeholder"}`}>
            {selected ? selected.label : placeholder}
          </span>
          {selected?.badge && <span className="combo-badge">{selected.badge}</span>}
        </span>
        <span className={`combo-chevron ${open ? "open" : ""}`}>
          <Icon name="down" size={14} />
        </span>
      </button>

      {open && (
        <div className="combo-popover" role="listbox">
          {searchable && (
            <div className="combo-search">
              <Icon name="search" size={14} />
              <input
                ref={inputRef}
                value={query}
                onChange={e => setQuery(e.target.value)}
                placeholder={searchPlaceholder}
              />
              {query && (
                <button
                  type="button"
                  className="combo-clear"
                  aria-label="清空搜索"
                  onClick={() => setQuery("")}
                >
                  <Icon name="close" size={12} />
                </button>
              )}
            </div>
          )}

          <div className="combo-list">
            {filtered.length === 0 ? (
              <div className="combo-empty">{emptyText}</div>
            ) : (
              filtered.map(opt => {
                const active = opt.value === value;
                return (
                  <button
                    key={opt.value}
                    type="button"
                    role="option"
                    aria-selected={active}
                    className={`combo-option ${active ? "active" : ""}`}
                    onClick={() => {
                      onChange(opt.value);
                      setOpen(false);
                    }}
                  >
                    {opt.leading && <span className="combo-leading">{opt.leading}</span>}
                    <span className="combo-option-text">
                      <span className="combo-option-title">
                        <strong>{opt.label}</strong>
                        {opt.badge && <span className="combo-badge">{opt.badge}</span>}
                      </span>
                      {opt.subtitle && <small>{opt.subtitle}</small>}
                    </span>
                    {active && (
                      <span className="combo-check">
                        <Icon name="check" size={14} />
                      </span>
                    )}
                  </button>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
}
