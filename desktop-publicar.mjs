#!/usr/bin/env node
// Registra una versión de Alfredo Caja de escritorio en el backend (F9 T9.10), INACTIVA.
// La llama el workflow desktop-release.yml tras subir el instalador a la release de GitHub, o
// se corre a mano (VM Windows o Mac) con el token de publicación:
//
//   DESKTOP_PUBLISH_TOKEN=… node tools/release/desktop-publicar.mjs \
//     --version 0.1.0 --url https://github.com/…/Alfredo-Caja_0.1.0_x64-setup.exe \
//     --file Alfredo-Caja_0.1.0_x64-setup.exe --sig Alfredo-Caja_0.1.0_x64-setup.exe.sig \
//     [--target windows|darwin] [--arch x86_64|aarch64|universal] [--download-url …dmg]
//     [--channel beta|stable] [--authenticode true|false] [--notes "…"]
//
// La versión se ACTIVA desde el control-plane (o PATCH /api/control/desktop-releases/…) tras
// la QA en Windows real (T9.12), y la política `escritorio/windows` decide latest/min/rollout.
import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]?.startsWith("--") ? "true" : all[i + 1]]] : acc), []),
);
const need = (k) => {
  if (!args[k]) {
    console.error(`falta --${k}`);
    process.exit(2);
  }
  return args[k];
};
const api = (process.env.API_BASE || "https://api.asistentealfredo.cl/api").replace(/\/+$/, "");
const token = process.env.DESKTOP_PUBLISH_TOKEN;
if (!token) {
  console.error("falta DESKTOP_PUBLISH_TOKEN");
  process.exit(2);
}
const file = need("file");
const body = {
  version: need("version"),
  target: args.target || "windows",
  arch: args.arch || "x86_64",
  url: need("url"),
  download_url: args["download-url"] || null,
  signature: readFileSync(need("sig"), "utf8").trim(),
  sha256: createHash("sha256").update(readFileSync(file)).digest("hex"),
  size_bytes: statSync(file).size,
  channel: args.channel === "stable" ? "stable" : "beta",
  authenticode_signed: args.authenticode === "true",
  notes: args.notes || null,
};
const res = await fetch(`${api}/control/desktop-releases`, {
  method: "POST",
  headers: { "content-type": "application/json", authorization: `Bearer ${token}` },
  body: JSON.stringify(body),
});
const text = await res.text();
if (!res.ok) {
  console.error(`El backend respondió ${res.status}: ${text}`);
  process.exit(1);
}
console.log(`Registrada Alfredo Caja ${body.version} (${body.target}/${body.arch}, ${body.channel}, inactiva). sha256 ${body.sha256}`);
