"use strict";

const fs = require("fs");
const path = require("path");
const zlib = require("zlib");

const root = path.resolve(__dirname, "..");
const sourcePath = path.join(root, "dark-dog-1.ico");
const outPath = path.join(root, "assets", "rundog.ico");

function readI32(buf, offset) {
  return buf.readInt32LE(offset);
}

function readU32(buf, offset) {
  return buf.readUInt32LE(offset);
}

function parseBmp32(buf) {
  if (buf.toString("ascii", 0, 2) !== "BM") {
    throw new Error("source is not a BMP");
  }
  const pixelsOffset = readU32(buf, 10);
  const width = readI32(buf, 18);
  const height = Math.abs(readI32(buf, 22));
  const bpp = buf.readUInt16LE(28);
  if (width !== 32 || height !== 32 || bpp !== 32) {
    throw new Error("expected 32x32 32-bit BMP");
  }
  const topDown = readI32(buf, 22) < 0;
  const rowBytes = width * 4;
  const pixels = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y += 1) {
    const srcY = topDown ? y : height - 1 - y;
    buf.copy(pixels, y * rowBytes, pixelsOffset + srcY * rowBytes, pixelsOffset + (srcY + 1) * rowBytes);
  }
  return { width, height, pixels };
}

function whiteDogOnBlack(src) {
  const pixels = Buffer.from(src.pixels);
  for (let i = 0; i < pixels.length; i += 4) {
    const alpha = pixels[i + 3];
    if (alpha !== 0) {
      pixels[i] = 255 - pixels[i];
      pixels[i + 1] = 255 - pixels[i + 1];
      pixels[i + 2] = 255 - pixels[i + 2];
    }
    const coverage = alpha / 255;
    pixels[i] = Math.round(pixels[i] * coverage);
    pixels[i + 1] = Math.round(pixels[i + 1] * coverage);
    pixels[i + 2] = Math.round(pixels[i + 2] * coverage);
    pixels[i + 3] = 255;
  }
  return { width: src.width, height: src.height, pixels };
}

function scaleNearest(src, size) {
  const out = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    const srcY = Math.min(src.height - 1, Math.floor((y * src.height) / size));
    for (let x = 0; x < size; x += 1) {
      const srcX = Math.min(src.width - 1, Math.floor((x * src.width) / size));
      src.pixels.copy(out, (y * size + x) * 4, (srcY * src.width + srcX) * 4, (srcY * src.width + srcX) * 4 + 4);
    }
  }
  return { width: size, height: size, pixels: out };
}

function roundCorners(src) {
  const radius = src.width * 0.22;
  const half = src.width / 2;
  const pixels = Buffer.from(src.pixels);
  for (let y = 0; y < src.height; y += 1) {
    for (let x = 0; x < src.width; x += 1) {
      const dx = Math.abs(x + 0.5 - half) - (half - radius);
      const dy = Math.abs(y + 0.5 - half) - (half - radius);
      const ox = Math.max(dx, 0);
      const oy = Math.max(dy, 0);
      const dist = Math.sqrt(ox * ox + oy * oy) + Math.min(Math.max(dx, dy), 0) - radius;
      const coverage = Math.max(0, Math.min(1, 0.5 - dist));
      const i = (y * src.width + x) * 4;
      pixels[i] = Math.round(pixels[i] * coverage);
      pixels[i + 1] = Math.round(pixels[i + 1] * coverage);
      pixels[i + 2] = Math.round(pixels[i + 2] * coverage);
      pixels[i + 3] = Math.round(pixels[i + 3] * coverage);
    }
  }
  return { width: src.width, height: src.height, pixels };
}

function crc32(buf) {
  let crc = 0xffffffff;
  for (let i = 0; i < buf.length; i += 1) {
    crc ^= buf[i];
    for (let j = 0; j < 8; j += 1) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const chunk = Buffer.alloc(8 + data.length + 4);
  chunk.writeUInt32BE(data.length, 0);
  chunk.write(type, 4, 4, "ascii");
  data.copy(chunk, 8);
  const crcBuf = Buffer.concat([Buffer.from(type), data]);
  chunk.writeUInt32BE(crc32(crcBuf), 8 + data.length);
  return chunk;
}

function encodePng(image) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(image.width, 0);
  ihdr.writeUInt32BE(image.height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const rows = [];
  for (let y = 0; y < image.height; y += 1) {
    const row = Buffer.alloc(1 + image.width * 4);
    for (let x = 0; x < image.width; x += 1) {
      const i = (y * image.width + x) * 4;
      // BMP BGRA -> PNG RGBA
      row[1 + x * 4] = image.pixels[i + 2];
      row[2 + x * 4] = image.pixels[i + 1];
      row[3 + x * 4] = image.pixels[i];
      row[4 + x * 4] = image.pixels[i + 3];
    }
    rows.push(row);
  }
  const idat = zlib.deflateSync(Buffer.concat(rows), { level: 9 });
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", idat),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

function encodeBmpIcon(image) {
  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0);
  header.writeInt32LE(image.width, 4);
  header.writeInt32LE(image.height * 2, 8);
  header.writeUInt16LE(1, 12);
  header.writeUInt16LE(32, 14);
  header.writeUInt32LE(0, 16);
  const xor = Buffer.alloc(image.width * image.height * 4);
  for (let y = 0; y < image.height; y += 1) {
    const srcY = image.height - 1 - y;
    image.pixels.copy(xor, y * image.width * 4, srcY * image.width * 4, (srcY + 1) * image.width * 4);
  }
  const andStride = Math.ceil(image.width / 32) * 4;
  const andMask = Buffer.alloc(andStride * image.height);
  for (let y = 0; y < image.height; y += 1) {
    const srcY = image.height - 1 - y;
    for (let x = 0; x < image.width; x += 1) {
      if (image.pixels[(srcY * image.width + x) * 4 + 3] === 0) {
        andMask[y * andStride + (x >> 3)] |= 0x80 >> (x & 7);
      }
    }
  }
  return Buffer.concat([header, xor, andMask]);
}

function buildIco(images) {
  const count = images.length;
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(count, 4);
  const entries = [];
  const blobs = [];
  let offset = 6 + 16 * count;
  for (const image of images) {
    const blob = image.width >= 256 ? encodePng(image) : encodeBmpIcon(image);
    const entry = Buffer.alloc(16);
    entry[0] = image.width >= 256 ? 0 : image.width;
    entry[1] = image.height >= 256 ? 0 : image.height;
    entry.writeUInt16LE(1, 4);
    entry.writeUInt16LE(32, 6);
    entry.writeUInt32LE(blob.length, 8);
    entry.writeUInt32LE(offset, 12);
    entries.push(entry);
    blobs.push(blob);
    offset += blob.length;
  }
  return Buffer.concat([header, ...entries, ...blobs]);
}

const source = whiteDogOnBlack(parseBmp32(fs.readFileSync(sourcePath)));
const sizes = [16, 24, 32, 48, 256];
const images = sizes.map((size) =>
  roundCorners(size === source.width ? source : scaleNearest(source, size)),
);
fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, buildIco(images));
console.log(`wrote ${outPath} (${fs.statSync(outPath).size} bytes)`);
