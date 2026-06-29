#!/usr/bin/env node
import { createWriteStream, existsSync, mkdirSync, chmodSync, readFileSync } from 'fs';
import { get } from 'https';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));

const pkg = JSON.parse(
  readFileSync(join(__dirname, 'package.json'), 'utf-8')
);

const PLATFORM_MAP = {
  'win32-x64': 'x86_64-pc-windows-msvc',
  'win32-arm64': 'aarch64-pc-windows-msvc',
  'darwin-x64': 'x86_64-apple-darwin',
  'darwin-arm64': 'aarch64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
};

const key = `${process.platform}-${process.arch}`;
const target = PLATFORM_MAP[key];

if (!target) {
  console.warn(`Unsupported platform: ${key}. Binary not found, but you can build from source.`);
  process.exit(0);
}

const ext = process.platform === 'win32' ? '.exe' : '';
const binaryName = `codebase-synapse-${target}${ext}`;
const destName = `codebase-synapse${ext}`;
const destPath = join(__dirname, destName);

// Skip if binary already exists
if (existsSync(destPath)) {
  console.log(`codebase-synapse binary already installed at ${destPath}`);
  process.exit(0);
}

const version = pkg.version;
const url = `https://github.com/codebase-synapse/index/releases/download/v${version}/${binaryName}`;

console.log(`Downloading codebase-synapse v${version} for ${key}...`);
console.log(`  ${url}`);

const file = createWriteStream(destPath);

await new Promise((resolve, reject) => {
  get(url, (response) => {
    if (response.statusCode === 302 || response.statusCode === 301) {
      get(response.headers.location, (r) => {
        r.pipe(file);
        file.on('finish', () => { file.close(); resolve(); });
      }).on('error', reject);
      return;
    }
    if (response.statusCode !== 200) {
      reject(new Error(`Download failed (HTTP ${response.statusCode}). Build from source: cargo build --release`));
      return;
    }
    response.pipe(file);
    file.on('finish', () => { file.close(); resolve(); });
  }).on('error', reject);
});

if (process.platform !== 'win32') {
  chmodSync(destPath, 0o755);
}

console.log(`Installed to ${destPath}`);
