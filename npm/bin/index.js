#!/usr/bin/env node
const { spawnSync } = require('child_process');
const path = require('path');
const fs = require('fs');

function getBinaryPath() {
  const platform = process.platform;
  const arch = process.arch === 'x64' ? 'x64' : 'arm64';
  const ext = platform === 'win32' ? '.exe' : '';

  // Try platform-specific package first
  const pkgName = `@codebase-synapse/index-${platform}-${arch}`;
  try {
    const pkgPath = require.resolve(pkgName);
    const binaryPath = require(pkgPath);
    if (fs.existsSync(binaryPath)) return binaryPath;
  } catch {}

  // Fallback: next to the script
  const localPath = path.join(__dirname, '..', `codebase-synapse${ext}`);
  if (fs.existsSync(localPath)) return localPath;

  // Fallback: PATH
  return `codebase-synapse${ext}`;
}

const binary = getBinaryPath();
const result = spawnSync(binary, process.argv.slice(2), {
  stdio: 'inherit',
  env: { ...process.env },
});

if (result.error) {
  console.error(`Failed to run codebase-synapse: ${result.error.message}`);
  console.error(`Make sure the binary is installed. Try: npm run postinstall`);
  process.exit(1);
}

process.exit(result.status ?? 0);
