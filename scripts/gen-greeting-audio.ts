// Membuat klip suara sapaan (neural Microsoft) ke public/greeting/ + manifest.json, untuk SEMUA mode suara
// di src/main-modules/greeting-lang.ts (cewek, cowok, anime cewek, anime cowok).
// Dijalankan SEKALI di PC pengembang (butuh internet), lalu klip ikut dibundel ke aplikasi.
// Ulangi bila daftar sapaan di greeting.ts atau preset suara berubah; klip yang sudah ada dilewati.
//
// kalimat dipecah per bahasa (Indonesia / Inggris / Prancis). Tiap potongan dibuat dengan voice native
// bahasanya, lalu diputar berurutan. Jadi "Welcome back, legend." dibaca aksen Inggris, bukan aksen Indonesia.
//
//   pip install edge-tts
//   npm run greeting:audio                                        # semua mode suara
//   $env:PRESETS="anime-cewek"; npm run greeting:audio            # PowerShell: hanya mode tertentu
//   npm run greeting:audio -- --dry                               # daftar teks & potongan, tanpa audio
//
// Opsi lingkungan: PRESETS, PYTHON (default python / python3), CONCURRENCY (default 2).
// Tambahkan `.cache/` ke .gitignore (cache potongan per bahasa).
import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import {
  copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, unlinkSync, writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { allSpokenTexts } from "../src/main-modules/greeting";
import { PRESETS, norm, splitByLanguage, type Seg, type VoicePreset } from "../src/main-modules/greeting-lang";

const run = promisify(execFile);
const PY = process.env.PYTHON ?? (process.platform === "win32" ? "python" : "python3");
const CONCURRENCY = Math.max(1, Number(process.env.CONCURRENCY) || 2);
const OUT = join(process.cwd(), "public", "greeting");
const SEG_CACHE = join(process.cwd(), ".cache", "greeting-seg");
const dry = process.argv.includes("--dry");

const only = (process.env.PRESETS ?? "").split(",").map((s) => s.trim()).filter(Boolean);
const presets = PRESETS.filter((p) => p.voices && (!only.length || only.includes(p.id)));
if (only.length && presets.length !== only.length) {
  console.error(`PRESETS tidak dikenal. Pilihan: ${PRESETS.filter((p) => p.voices).map((p) => p.id).join(", ")}`);
  process.exit(1);
}

const sign = (n: number, unit: string) => `${n >= 0 ? "+" : ""}${n}${unit}`;
const sha = (s: string, n: number) => createHash("sha1").update(s).digest("hex").slice(0, n);

type Job = { path: string; text: string; voice: string; rate: string; pitch: string };
const jobFor = (p: VoicePreset, seg: Seg): Job => {
  const voice = p.voices![seg.lang];
  const rate = sign(p.ratePct, "%");
  const pitch = sign(p.pitchHz[seg.lang], "Hz");
  return { path: join(SEG_CACHE, sha(`${voice}|${rate}|${pitch}|${seg.text}`, 16) + ".mp3"), text: seg.text, voice, rate, pitch };
};
const outputFile = (j: Job) => sha(`${j.voice}|${j.rate}|${j.pitch}|${j.text}`, 16) + ".mp3";

async function synth(j: Job) {
  for (let attempt = 1; ; attempt++) {
    try {
      await run(PY, ["-m", "edge_tts", "--voice", j.voice, `--rate=${j.rate}`, `--pitch=${j.pitch}`, `--text=${j.text}`, "--write-media", j.path]);
      if (!existsSync(j.path) || statSync(j.path).size === 0) throw new Error(`Edge TTS membuat audio kosong: ${j.path}`);
      return;
    } catch (e) {
      if (attempt >= 5) throw e;
      if (existsSync(j.path)) unlinkSync(j.path);
      await new Promise((r) => setTimeout(r, 2500 * attempt));
    }
  }
}

async function pool<T>(items: T[], n: number, fn: (t: T, i: number) => Promise<void>) {
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, async () => {
    while (next < items.length) { const k = next++; await fn(items[k], k); }
  }));
}

async function main() {
  const texts = [...new Set(allSpokenTexts().map(norm))];
  mkdirSync(OUT, { recursive: true });
  mkdirSync(SEG_CACHE, { recursive: true });

  // 1. rencana: tiap (preset, kalimat) -> potongan per bahasa
  const plan: { preset: VoicePreset; text: string; jobs: Job[] }[] = [];
  const segJobs = new Map<string, Job>();
  for (const preset of presets) {
    for (const text of texts) {
      const jobs = splitByLanguage(text).map((s) => jobFor(preset, s));
      jobs.forEach((j) => segJobs.set(j.path, j));
      plan.push({ preset, text, jobs });
    }
  }

  if (dry) {
    for (const p of presets) console.log(`\n== ${p.id}: ${p.label}`);
    for (const t of texts) console.log(t, "\n   ", splitByLanguage(t).map((s) => `[${s.lang}] ${s.text}`).join("  "));
    console.log(`\n${texts.length} kalimat x ${presets.length} mode = ${plan.length} klip, ${segJobs.size} potongan unik (dry run).`);
    return;
  }

  // 2. buat potongan yang belum ada di cache
  const todo = [...segJobs.values()].filter((j) => !existsSync(j.path) || statSync(j.path).size === 0);
  await pool(todo, CONCURRENCY, async (j, i) => {
    console.log(`[potongan ${i + 1}/${todo.length}] (${j.voice}) ${j.text}`);
    await synth(j);
  });

  // 3. Salin potongan berbahasa ke aset dan catat urutan putarnya.
  const manifestPath = join(OUT, "manifest.json");
  let manifest: Record<string, string[]> = {};
  try { manifest = JSON.parse(readFileSync(manifestPath, "utf8")); } catch { /* belum ada */ }
  for (const k of Object.keys(manifest)) if (presets.some((p) => k.startsWith(p.id + "|"))) delete manifest[k];

  let made = 0;
  for (const job of segJobs.values()) {
    const path = join(OUT, outputFile(job));
    if (existsSync(path) && statSync(path).size > 0) continue;
    copyFileSync(job.path, path);
    made++;
  }
  for (const { preset, text, jobs } of plan)
    manifest[`${preset.id}|${text}`] = jobs.map(outputFile);
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 1));

  // 4. hapus klip yatim (tidak dipakai manifest mana pun)
  const used = new Set(Object.values(manifest).flat());
  let removed = 0;
  for (const f of readdirSync(OUT)) if (f.endsWith(".mp3") && !used.has(f)) { unlinkSync(join(OUT, f)); removed++; }

  console.log(`Selesai: ${made} potongan audio baru, ${Object.keys(manifest).length} entri manifest di ${OUT}` + (removed ? `, ${removed} klip lama dihapus` : ""));
}

main().catch((e) => {
  console.error("Gagal membuat klip. Pastikan `pip install edge-tts` sudah dijalankan dan ada internet.\n", e);
  process.exit(1);
});
