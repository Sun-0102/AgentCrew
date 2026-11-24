import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const rootDir = process.cwd();
const releasesDir = path.join(rootDir, "releases");
fs.mkdirSync(releasesDir, { recursive: true });

if (process.platform === "darwin") {
  execSync("npx tauri build --config src-tauri/tauri.macos.conf.json", {
    stdio: "inherit",
  });
  const srcApp = path.join(
    rootDir,
    "src-tauri/target/release/bundle/macos/AgentCrew.app",
  );
  const destApp = path.join(releasesDir, "AgentCrew.app");
  fs.rmSync(destApp, { recursive: true, force: true });
  execSync(`ditto "${srcApp}" "${destApp}"`, { stdio: "inherit" });
  console.log(`\n[AgentCrew] 已更新 macOS 应用: ${destApp}`);

  const dmgDir = path.join(rootDir, "src-tauri/target/release/bundle/dmg");
  if (fs.existsSync(dmgDir)) {
    const dmgs = fs.readdirSync(dmgDir).filter((name) => name.endsWith(".dmg"));
    for (const file of dmgs) {
      const dest = path.join(releasesDir, file);
      fs.rmSync(dest, { force: true });
      fs.copyFileSync(path.join(dmgDir, file), dest);
      console.log(`[AgentCrew] 已更新 macOS 安装包: ${dest}`);
    }
  }
} else if (process.platform === "win32") {
  execSync("npx tauri build", { stdio: "inherit" });
  const nsisDir = path.join(rootDir, "src-tauri/target/release/bundle/nsis");
  const installers = fs
    .readdirSync(nsisDir)
    .filter((name) => name.endsWith(".exe"));
  for (const file of installers) {
    const dest = path.join(releasesDir, file);
    fs.rmSync(dest, { force: true });
    fs.copyFileSync(path.join(nsisDir, file), dest);
    console.log(`\n[AgentCrew] 已更新 Windows 安装包: ${dest}`);
  }
} else {
  execSync("npx tauri build", { stdio: "inherit" });
  console.log(`\n[AgentCrew] 构建完成，产物位于 src-tauri/target/release/bundle/`);
}
