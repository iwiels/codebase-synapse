#!/usr/bin/env node
const { spawn } = require('child_process');
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
const child = spawn(binary, process.argv.slice(2), {
  stdio: 'inherit',
  env: { ...process.env },
});

child.on('close', (code) => {
  process.exit(code ?? 0);
});

child.on('error', (err) => {
  console.error(`Failed to run codebase-synapse: ${err.message}`);
  console.error(`Make sure the binary is installed. Try: npm run postinstall`);
  process.exit(1);
});

// Forward termination signals to ensure the Rust process exits cleanly
const signals = ['SIGINT', 'SIGTERM', 'SIGQUIT', 'SIGHUP'];
signals.forEach((sig) => {
  process.on(sig, () => {
    if (!child.killed) {
      child.kill(sig);
    }
    // Safety exit in case the child process gets stuck
    setTimeout(() => {
      process.exit(1);
    }, 2000).unref();
  });
});
