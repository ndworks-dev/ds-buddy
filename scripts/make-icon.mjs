// Renders ui/icon.svg into ui/icon.png (the window icon) and icon.ico (the
// .exe icon). The ICO holds PNG-compressed images, which Windows Vista+ reads,
// at the sizes Explorer and the taskbar ask for, so small sizes are rendered
// from the vector instead of shrunk from 256 px.
//
// Needs Node and sharp, installed globally so the repository gets no
// node_modules. From the repository root:
//
//   npm install -g sharp
//   NODE_PATH="$(npm root -g)" node scripts/make-icon.mjs        # bash
//   $env:NODE_PATH = npm root -g; node scripts/make-icon.mjs     # PowerShell

import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

// `require` honors NODE_PATH, which `import` does not.
const sharp = createRequire(import.meta.url)("sharp");

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const svg = readFileSync(join(root, "ui", "icon.svg"));
const sizes = [16, 20, 24, 32, 40, 48, 64, 256];

const render = (size) => sharp(svg, { density: (72 * size) / 256 }).resize(size, size).png().toBuffer();

const pngs = await Promise.all(sizes.map(render));
writeFileSync(join(root, "ui", "icon.png"), pngs[sizes.indexOf(256)]);

// ICONDIR, then one ICONDIRENTRY per image, then the images.
const header = Buffer.alloc(6 + 16 * sizes.length);
header.writeUInt16LE(0, 0); // reserved
header.writeUInt16LE(1, 2); // type: icon
header.writeUInt16LE(sizes.length, 4);
let offset = header.length;
sizes.forEach((size, i) => {
  const entry = 6 + 16 * i;
  header.writeUInt8(size % 256, entry); // width, 0 means 256
  header.writeUInt8(size % 256, entry + 1); // height
  header.writeUInt8(0, entry + 2); // no palette
  header.writeUInt8(0, entry + 3); // reserved
  header.writeUInt16LE(1, entry + 4); // color planes
  header.writeUInt16LE(32, entry + 6); // bits per pixel
  header.writeUInt32LE(pngs[i].length, entry + 8);
  header.writeUInt32LE(offset, entry + 12);
  offset += pngs[i].length;
});
writeFileSync(join(root, "icon.ico"), Buffer.concat([header, ...pngs]));

console.log("Wrote ui/icon.png and icon.ico");
