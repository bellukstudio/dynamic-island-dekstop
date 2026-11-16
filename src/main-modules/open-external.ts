// Membuka URL di browser bawaan sistem. window.open() di WebView Tauri tidak membuka browser,
// jadi dicoba beberapa jalur Tauri berurutan sampai ada yang berhasil:
//  1. command Rust "open_url" (src-tauri/open_url.rs, tanpa dependensi tambahan),
//  2. plugin opener Tauri v2 ("plugin:opener|open_url"),
//  3. plugin shell Tauri v2 ("plugin:shell|open").
import { invoke } from "@tauri-apps/api/core";

export type OpenResult = { ok: boolean; via?: string; error?: string };

export async function openExternalUrl(url: string): Promise<OpenResult> {
  const attempts: Array<[string, () => Promise<unknown>]> = [
    ["open_url", () => invoke("open_url", { url })],
    ["plugin:opener", () => invoke("plugin:opener|open_url", { url })],
    ["plugin:shell", () => invoke("plugin:shell|open", { path: url })],
  ];
  const errors: string[] = [];
  for (const [name, run] of attempts) {
    try {
      await run();
      return { ok: true, via: name };
    } catch (error) {
      errors.push(`${name}: ${String(error)}`);
    }
  }
  return { ok: false, error: errors.join(" | ") };
}
