// 生成占位应用图标：墨底 + 素环 + 朱色书脊。
// 用法：node scripts/gen-icon.mjs  → 再执行 pnpm tauri icon app-icon.png
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const SIZE = 1024;
const BG = [0x15, 0x15, 0x1d];
const RING = [0xcf, 0xc8, 0xb8];
const SPINE = [0xc0, 0x4a, 0x38];

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));

function roundedBox(cx, cy, halfW, halfH, radius) {
  const qx = Math.abs(cx) - (halfW - radius);
  const qy = Math.abs(cy) - (halfH - radius);
  const outside = Math.hypot(Math.max(qx, 0), Math.max(qy, 0));
  return outside + Math.min(Math.max(qx, qy), 0) - radius;
}

function box(cx, cy, halfW, halfH) {
  const qx = Math.abs(cx) - halfW;
  const qy = Math.abs(cy) - halfH;
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0);
}

// coverage: SDF 距离转覆盖率，1px 抗锯齿
const cover = (d) => clamp(0.5 - d, 0, 1);

const px = Buffer.alloc(SIZE * SIZE * 4, 0);

for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const cx = x + 0.5 - SIZE / 2;
    const cy = y + 0.5 - SIZE / 2;

    const bgA = cover(roundedBox(cx, cy, 504, 504, 190));
    if (bgA <= 0) continue;

    let rgb = BG;
    let alpha = bgA;

    const ringA = cover(Math.abs(Math.hypot(cx, cy) - 300) - 11);
    if (ringA > 0) rgb = mix(rgb, RING, ringA);

    const spineA = cover(box(cx, cy, 15, 332));
    if (spineA > 0) rgb = mix(rgb, SPINE, spineA);

    const i = (y * SIZE + x) * 4;
    px[i] = rgb[0];
    px[i + 1] = rgb[1];
    px[i + 2] = rgb[2];
    px[i + 3] = Math.round(alpha * 255);
  }
}

// 每行前置 filter 字节 0
const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0;
  px.copy(raw, y * (SIZE * 4 + 1) + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
}

const crcTable = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

writeFileSync(new URL("../app-icon.png", import.meta.url), png);
console.log("已生成 app-icon.png", png.length, "字节");
