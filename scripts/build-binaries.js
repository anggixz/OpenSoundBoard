import { execSync } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");

const mode = process.argv[2] || "all"; // "portable", "installer", or "all"

const outputDir = path.join(projectRoot, "dist-bin");
const portableDir = path.join(outputDir, "portable");
const installerDir = path.join(outputDir, "installer");

fs.mkdirSync(portableDir, { recursive: true });
fs.mkdirSync(installerDir, { recursive: true });

console.log(`\n==================================================`);
console.log(`[OpenSoundBoard] Building native Windows binaries (${mode})`);
console.log(`==================================================\n`);

function safeCopy(src, dest) {
  try {
    fs.copyFileSync(src, dest);
  } catch (err) {
    if (err.code === "EBUSY") {
      try {
        execSync("taskkill /F /IM OpenSoundBoard.exe", { stdio: "ignore" });
      } catch (_) {}
      fs.copyFileSync(src, dest);
    } else {
      throw err;
    }
  }
}

function buildPortable() {
  console.log(`\n[1/2] 🔨 Membangun Portable Executable (.exe)...`);
  execSync("npm run build", { cwd: projectRoot, stdio: "inherit" });
  execSync("npx tauri build --no-bundle", { cwd: projectRoot, stdio: "inherit" });

  const srcExe = path.join(projectRoot, "src-tauri", "target", "release", "OpenSoundBoard.exe");
  const destExe = path.join(portableDir, "OpenSoundBoard.exe");

  if (fs.existsSync(srcExe)) {
    safeCopy(srcExe, destExe);
    const stats = fs.statSync(destExe);
    const sizeMb = (stats.size / (1024 * 1024)).toFixed(2);
    console.log(`\n✅ Portable EXE berhasil dibuat:`);
    console.log(`   Path : ${destExe}`);
    console.log(`   Size : ${sizeMb} MB\n`);
  } else {
    console.error(`❌ Gagal menemukan executable di ${srcExe}`);
  }
}

function buildInstaller() {
  console.log(`\n[2/2] 🔨 Membangun NSIS Installer Setup (.exe)...`);
  execSync("npm run build", { cwd: projectRoot, stdio: "inherit" });
  execSync("npx tauri build --bundles nsis", { cwd: projectRoot, stdio: "inherit" });

  const nsisDir = path.join(projectRoot, "src-tauri", "target", "release", "bundle", "nsis");
  if (fs.existsSync(nsisDir)) {
    const files = fs.readdirSync(nsisDir).filter(f => f.endsWith(".exe"));
    for (const f of files) {
      const srcFile = path.join(nsisDir, f);
      const destFile = path.join(installerDir, f);
      safeCopy(srcFile, destFile);
      const stats = fs.statSync(destFile);
      const sizeMb = (stats.size / (1024 * 1024)).toFixed(2);
      console.log(`\n✅ Installer Setup EXE berhasil dibuat:`);
      console.log(`   Path : ${destFile}`);
      console.log(`   Size : ${sizeMb} MB\n`);
    }
  } else {
    console.error(`❌ Gagal menemukan direktori bundle nsis di ${nsisDir}`);
  }
}

if (mode === "portable") {
  buildPortable();
} else if (mode === "installer") {
  buildInstaller();
} else {
  buildPortable();
  buildInstaller();
}

console.log(`\n==================================================`);
console.log(`✨ Selesai! Semua biner tersedia di: ${outputDir}`);
console.log(`==================================================\n`);
