import { useMemo } from "react";
import { MultiFileDiff } from "@pierre/diffs/react";
import type { GitFileDiff } from "./api";

interface Props {
  diff: GitFileDiff | null;
  loading: boolean;
  diffStyle: "split" | "unified";
  wrap: boolean;
}

export default function DiffViewer({ diff, loading, diffStyle, wrap }: Props) {
  const options = useMemo(
    () => ({
      diffStyle,
      overflow: wrap ? ("wrap" as const) : ("scroll" as const),
      themeType: "light" as const,
      disableFileHeader: false,
      diffIndicators: "bars" as const,
    }),
    [diffStyle, wrap],
  );

  if (loading) {
    return (
      <div className="git-diff-empty">
        <span className="small-spinner" />
        <span>正在加载文件差异…</span>
      </div>
    );
  }

  if (!diff) {
    return (
      <div className="git-diff-empty">
        <strong>选择左侧文件查看代码变更</strong>
        <p>支持 Split 双栏对比与 Unified 单栏视图，语法高亮由 @pierre/diffs 驱动。</p>
      </div>
    );
  }

  if (diff.is_binary) {
    return (
      <div className="git-diff-empty">
        <strong>二进制文件变更</strong>
        <p>{diff.file_path} 为二进制文件，暂不支持文本 Diff 预览。</p>
      </div>
    );
  }

  if (diff.old_contents === diff.new_contents && !diff.patch) {
    return (
      <div className="git-diff-empty">
        <strong>当前视图下无文本差异</strong>
        <p>可切换上方的「全部变更 / 未暂存 / 已暂存」模式查看其他阶段的修改。</p>
      </div>
    );
  }

  return (
    <div className="git-diff-canvas">
      {diff.truncated && (
        <div className="git-diff-banner">
          文件内容超过 512 KB，已截取前部内容用于快速预览。
        </div>
      )}
      <MultiFileDiff
        key={`${diff.file_path}:${diffStyle}:${wrap ? "wrap" : "scroll"}:${diff.old_contents.length}:${diff.new_contents.length}`}
        oldFile={{
          name: diff.file_path,
          contents: diff.old_contents,
        }}
        newFile={{
          name: diff.file_path,
          contents: diff.new_contents,
        }}
        options={options}
        disableWorkerPool
      />
    </div>
  );
}
